//! Oracle contract — the on-chain handler for the BTC/ETH multi-round TWAP demo.
//!
//! Round flow (the on-chain side):
//!
//! 1. A user calls `request_twap(asset, range_secs)` from the UI. The contract
//!    assigns a fresh `request_id`, persists `RequestInfo`, and emits a
//!    `TwapRequest` Soroban event. Every Vectr's *Round 2* circuit is
//!    triggered by this event.
//!
//! 2. Each Vectr computes its own geometric TWAP over its locally-stored
//!    CoinGecko samples and submits a SINGLE-signer envelope through the
//!    aggregator. The contract validates that signer via
//!    `Ed25519VerificationClient::check_one` (which does NOT require quorum
//!    — exact-match attestations are impossible because two Vectrs can never
//!    fetch CoinGecko at the same instant) and appends to a per-request
//!    bundle. When the bundle reaches the configured quorum threshold the
//!    contract emits a `Round2Ready` Soroban event carrying the full bundle
//!    — this is the "composition event" the *Round 3* (median) circuits
//!    listen on.
//!
//! 3. Each Vectr's *Round 3* circuit decodes the bundle, validates every
//!    inner signature against the security module (off-chain, via WASI
//!    Soroban reads), takes the median of the valid TWAPs, and submits a
//!    QUORUM-signed final envelope through the same aggregator. The
//!    contract validates the envelope via `Ed25519VerificationClient::verify`
//!    (full threshold) and stores the median.
//!
//! See `OracleContract::submit_round2` for the single-signer entry and
//! `OracleContract::submit_final` for the quorum entry.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::FromXdr, Address,
    Bytes, BytesN, Env, String, Symbol, Vec,
};
use warpdrive_shared::interfaces::{
    handler::{Ed25519SignatureData, XlmEnvelope},
    verification::Ed25519VerificationClient,
};

use crate::storage::{self, RequestInfo};

// ─── error type ───────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum OracleError {
    InvalidEnvelope = 1,
    InvalidRound2Payload = 2,
    InvalidFinalPayload = 3,
    EventAlreadySeen = 4,
    UnknownVerificationError = 5,
    OtherInvocationError = 6,
    SignerMismatch = 7,
    DuplicateAttestation = 8,
    UnknownRequest = 9,
    AlreadyFinalized = 10,
    RoundNotReady = 11,
    InvalidSignature = 12,
    SignerNotRegistered = 13,
    InsufficientWeight = 14,
    SignersNotOrdered = 15,
    EmptySignatures = 16,
    LengthMismatch = 17,
    ZeroRequiredWeight = 18,
    QuorumOutOfRange = 19,
}

impl From<warpdrive_shared::interfaces::verification::VerifyError> for OracleError {
    fn from(e: warpdrive_shared::interfaces::verification::VerifyError) -> Self {
        use warpdrive_shared::interfaces::verification::VerifyError as V;
        match e {
            V::InvalidSignature => OracleError::InvalidSignature,
            V::SignerNotRegistered => OracleError::SignerNotRegistered,
            V::InsufficientWeight => OracleError::InsufficientWeight,
            V::EmptySignatures => OracleError::EmptySignatures,
            V::LengthMismatch => OracleError::LengthMismatch,
            V::SignersNotOrdered => OracleError::SignersNotOrdered,
            V::ZeroRequiredWeight => OracleError::ZeroRequiredWeight,
        }
    }
}

// ─── public payload types ─────────────────────────────────────────────

/// The XDR payload a Vectr's Round 2 circuit emits, after the host signs
/// the surrounding `XlmEnvelope`. Mirrors the off-chain Rust struct
/// produced by `twap-circuit`.
#[contracttype]
#[derive(Clone)]
pub struct Round2Payload {
    pub request_id: u64,
    pub asset: Symbol,
    pub range_secs: u32,
    /// Geometric TWAP scaled to 7 decimals (e.g. 67_123_4567 ≈ 67.123 USD).
    /// We use `i128` so the same type can carry asset prices in the
    /// dollar-millions range (BTC) and the dollar range (XLM-derived
    /// stablecoins) without overflow.
    pub twap: i128,
    /// Wall-clock at which the Vectr computed this — included so the
    /// median circuit can prefer fresher attestations on ties.
    pub computed_at: u64,
}

/// The XDR payload a Vectr's Round 3 (composition) circuit emits.
#[contracttype]
#[derive(Clone)]
pub struct FinalPayload {
    pub request_id: u64,
    pub asset: Symbol,
    /// Median of the valid Round 2 TWAPs, 7-decimal-scaled.
    pub median: i128,
    /// How many attestations contributed to the median (after signature
    /// validation in the Vectr).
    pub n_attestations: u32,
    pub computed_at: u64,
}

/// The XDR payload the `eth-bridge-circuit` emits when it observes a
/// `TwapRequested(string,uint32,address)` event on Sepolia. Unlike
/// `Round2Payload`/`FinalPayload` this carries no `request_id`: the
/// contract mints a fresh one on accept, so the bridged request joins
/// the same Round-2/3 pipeline as a Stellar-native `request_twap` call.
///
/// Field order, types, and `Option` wrapping are LOCKED — both the
/// bridge circuit (XDR-encodes this on the host side) and the
/// frontend (filters `twapreq` events by `originator`) depend on the
/// byte-for-byte layout.
#[contracttype]
#[derive(Clone)]
pub struct BridgeTriggerPayload {
    pub asset: Symbol,
    pub range_secs: u32,
    /// `msg.sender` from the Sepolia `TwapRequested` event — the
    /// MetaMask wallet that paid Sepolia gas for the original
    /// trigger.
    pub eth_origin: BytesN<20>,
    /// Sepolia transaction hash, used by the host as the
    /// `event_id_salt` so both operators collapse to the same
    /// `event_id` and the contract dedups replays via
    /// `is_event_seen`.
    pub eth_tx_hash: BytesN<32>,
    /// `log.block_timestamp` from Sepolia — deterministic across
    /// operators, so it can't drift between the two witnesses.
    pub block_timestamp: u64,
}

/// Discriminated payload the off-chain Vectrs put inside the signed
/// envelope. The warpdrive node's submission manager always invokes
/// `verify_xlm` on the handler — there is no per-round entrypoint to
/// route on — so we wrap each round's struct in a tagged enum and
/// dispatch on the variant inside the handler.
#[contracttype]
#[derive(Clone)]
pub enum SubmissionPayload {
    /// Single-Vectr Round 2 attestation: this Vectr's geometric TWAP.
    Round2(Round2Payload),
    /// Quorum-signed Round 3 attestation: the median of the Round 2
    /// bundle.
    Final(FinalPayload),
    /// Quorum-signed observation of a `TwapRequested` event on the
    /// configured EVM chain — both operators must agree they saw the
    /// same Sepolia log before the contract mints a fresh request_id
    /// and emits the usual `twapreq` Soroban event.
    BridgeTrigger(BridgeTriggerPayload),
}

/// One Vectr's Round 2 contribution as recorded on chain — what's bundled
/// in the `Round2Ready` event for downstream composition circuits.
#[contracttype]
#[derive(Clone)]
pub struct Round2Attestation {
    pub signer: BytesN<32>,
    pub signature: BytesN<64>,
    pub envelope: Bytes,
    pub twap: i128,
    pub computed_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct Round2Bundle {
    pub attestations: Vec<Round2Attestation>,
}

#[contracttype]
#[derive(Clone)]
pub struct LatestTwap {
    pub twap: i128,
    pub ts: u64,
}

// ─── contract ─────────────────────────────────────────────────────────

#[contract]
pub struct OracleContract;

#[contractimpl]
impl OracleContract {
    /// Wires the contract to the project's ed25519 verification module
    /// and sets the initial quorum fraction (defaults to 4/5 = 80%, the
    /// figure the demo spec calls out).
    pub fn __constructor(
        env: Env,
        verification_contract: Address,
        quorum_numerator: u32,
        quorum_denominator: u32,
    ) -> Result<(), OracleError> {
        if quorum_denominator == 0
            || quorum_numerator == 0
            || quorum_numerator > quorum_denominator
        {
            return Err(OracleError::QuorumOutOfRange);
        }
        storage::set_verification_contract(&env, &verification_contract);
        storage::set_quorum(&env, quorum_numerator, quorum_denominator);
        storage::set_version(&env, &String::from_str(&env, env!("CARGO_PKG_VERSION")));
        storage::extend_instance_ttl(&env);
        Ok(())
    }

    // ── public, anyone-can-call ──────────────────────────────────────

    /// User-facing entry: ask the Vectr swarm for a TWAP on `asset` over
    /// `range_secs`. Returns the request id (clients store it to poll
    /// for the final result via `final_twap(id)`).
    pub fn request_twap(env: Env, asset: Symbol, range_secs: u32) -> u64 {
        let id = storage::next_request_id(&env);
        let now = env.ledger().timestamp();
        let info = RequestInfo {
            asset: asset.clone(),
            range_secs,
            requested_at: now,
            // Stellar-native path: no EVM origin to record.
            origin: None,
        };
        storage::save_request(&env, id, &info);
        storage::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("twapreq"), id),
            TwapRequestData {
                asset,
                range_secs,
                requested_at: now,
                originator: None,
            },
        );
        id
    }

    // ── handler entry — the warpdrive node submits everything here ───

    /// `StellarHandlerInterface::verify_xlm` — the single entry the
    /// warpdrive node's submission manager invokes for every
    /// aggregated submission against this handler. The envelope payload
    /// is a tagged `SubmissionPayload` whose variant decides whether
    /// this is a Round 2 single-Vectr attestation or a Round 3
    /// quorum-signed final.
    ///
    /// Both variants go through the same `try_verify` call (the
    /// quorum is global to the security contract; with a single
    /// operator at 1/1, one sig satisfies it; with N at 4/5, Round 2
    /// payloads need their `event_id_salt` to be unique per Vectr so
    /// the host doesn't try to quorum-collapse different-value
    /// attestations).
    pub fn verify_xlm(
        env: Env,
        envelope_bytes: Bytes,
        sig_data: Ed25519SignatureData,
    ) -> Result<(), OracleError> {
        let envelope = XlmEnvelope::from_xdr(&env, &envelope_bytes)
            .map_err(|_| OracleError::InvalidEnvelope)?;
        let event_id = envelope.event_id.clone();
        if storage::is_event_seen(&env, &event_id) {
            return Err(OracleError::EventAlreadySeen);
        }

        // Decode the payload BEFORE verifying so we know which crypto
        // check the variant wants:
        //
        // * Round 2 is per-Vectr — each operator's twap differs (CoinGecko
        //   drift), so the host can't quorum-collapse them. We accept
        //   single-signer envelopes via `try_check_one`, which only
        //   checks the sig is from a registered signer with non-zero
        //   weight. The bundle on chain accumulates each one until the
        //   release threshold is crossed.
        //
        // * Round 3 (median) is deterministic — every operator runs the
        //   same median over the same bundle, so the host quorum-
        //   collapses them into one envelope with N signatures.
        //   `try_verify` enforces sum-of-weights ≥ required_weight at
        //   the reference_block.
        let payload = SubmissionPayload::from_xdr(&env, &envelope.payload)
            .map_err(|_| OracleError::InvalidEnvelope)?;
        let verification_addr = storage::get_verification_contract(&env);
        let verification = Ed25519VerificationClient::new(&env, &verification_addr);

        match payload {
            SubmissionPayload::Round2(p) => {
                if sig_data.signatures.len() != 1 || sig_data.signers.len() != 1 {
                    return Err(OracleError::LengthMismatch);
                }
                let signer = sig_data.signers.get(0).expect("len==1");
                let signature = sig_data.signatures.get(0).expect("len==1");
                match verification.try_check_one(
                    &envelope_bytes,
                    &signature,
                    &signer,
                    &Some(sig_data.reference_block),
                ) {
                    Ok(Ok(_weight)) => {}
                    Ok(Err(_)) => return Err(OracleError::UnknownVerificationError),
                    Err(Ok(e)) => return Err(OracleError::from(e)),
                    Err(Err(_)) => return Err(OracleError::OtherInvocationError),
                }
                Self::apply_round2(
                    &env,
                    &verification_addr,
                    &signer,
                    &signature,
                    &envelope_bytes,
                    &event_id,
                    p,
                )
            }
            SubmissionPayload::Final(p) => {
                match verification.try_verify(
                    &envelope_bytes,
                    &sig_data.signatures,
                    &sig_data.signers,
                    &sig_data.reference_block,
                ) {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => return Err(OracleError::UnknownVerificationError),
                    Err(Ok(e)) => return Err(OracleError::from(e)),
                    Err(Err(_)) => return Err(OracleError::OtherInvocationError),
                }
                Self::apply_final(&env, &event_id, p)
            }
            SubmissionPayload::BridgeTrigger(p) => {
                // Full quorum: both operators must independently witness
                // the same Sepolia `TwapRequested` log and produce
                // identical `BridgeTriggerPayload` bytes, so the host
                // quorum-collapses their signatures into one envelope.
                // `try_verify` enforces sum-of-weights ≥ required at
                // the reference_block, same as the `Final` arm.
                match verification.try_verify(
                    &envelope_bytes,
                    &sig_data.signatures,
                    &sig_data.signers,
                    &sig_data.reference_block,
                ) {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => return Err(OracleError::UnknownVerificationError),
                    Err(Ok(e)) => return Err(OracleError::from(e)),
                    Err(Err(_)) => return Err(OracleError::OtherInvocationError),
                }
                Self::apply_bridge_trigger(&env, &event_id, p)
            }
        }
    }

    // ── internal round 2 logic ────────────────────────────────────────

    fn apply_round2(
        env: &Env,
        verification_addr: &Address,
        signer: &BytesN<32>,
        signature: &BytesN<64>,
        envelope_bytes: &Bytes,
        event_id: &BytesN<20>,
        payload: Round2Payload,
    ) -> Result<(), OracleError> {

        let info = storage::get_request(env, payload.request_id)
            .ok_or(OracleError::UnknownRequest)?;
        if info.asset != payload.asset || info.range_secs != payload.range_secs {
            return Err(OracleError::InvalidRound2Payload);
        }
        if storage::get_final(env, payload.request_id).is_some() {
            return Err(OracleError::AlreadyFinalized);
        }

        // Dedup: one attestation per Vectr per request.
        let mut bundle = storage::load_bundle(env, payload.request_id);
        for existing in bundle.attestations.iter() {
            if &existing.signer == signer {
                return Err(OracleError::DuplicateAttestation);
            }
        }
        bundle.attestations.push_back(Round2Attestation {
            signer: signer.clone(),
            signature: signature.clone(),
            envelope: envelope_bytes.clone(),
            twap: payload.twap,
            computed_at: payload.computed_at,
        });
        storage::save_bundle(env, payload.request_id, &bundle);
        storage::mark_event_seen(env, event_id);
        storage::extend_instance_ttl(env);

        // Threshold release: once we hit (or pass) ceil(total_signers * num/denom),
        // emit the composition event exactly once.
        if !storage::round2_released(env, payload.request_id) {
            let total = security_signer_count(env, verification_addr);
            let threshold = ceil_div(
                total * storage::get_quorum_numerator(env),
                storage::get_quorum_denominator(env),
            );
            if bundle.attestations.len() >= threshold {
                storage::mark_round2_released(env, payload.request_id);
                env.events().publish(
                    (symbol_short!("r2ready"), payload.request_id),
                    Round2ReadyData {
                        asset: payload.asset.clone(),
                        range_secs: payload.range_secs,
                        bundle: bundle.clone(),
                    },
                );
            }
        }
        Ok(())
    }

    // ── internal final logic ──────────────────────────────────────────

    fn apply_final(
        env: &Env,
        event_id: &BytesN<20>,
        payload: FinalPayload,
    ) -> Result<(), OracleError> {
        let info = storage::get_request(env, payload.request_id)
            .ok_or(OracleError::UnknownRequest)?;
        if info.asset != payload.asset {
            return Err(OracleError::InvalidFinalPayload);
        }
        if !storage::round2_released(env, payload.request_id) {
            return Err(OracleError::RoundNotReady);
        }
        if storage::get_final(env, payload.request_id).is_some() {
            return Err(OracleError::AlreadyFinalized);
        }

        storage::save_final(
            env,
            payload.request_id,
            &payload.asset,
            payload.median,
            payload.computed_at,
        );
        storage::mark_event_seen(env, event_id);
        storage::extend_instance_ttl(env);

        env.events().publish(
            (symbol_short!("finaltwp"), payload.request_id),
            FinalizedData {
                asset: payload.asset,
                median: payload.median,
                n_attestations: payload.n_attestations,
                computed_at: payload.computed_at,
            },
        );
        Ok(())
    }

    // ── internal bridge trigger logic ─────────────────────────────────

    /// Mints a fresh request_id for an EVM-bridged `TwapRequested` event,
    /// persists it with the originating wallet attached, and emits the
    /// same `twapreq` Soroban event the Stellar-native `request_twap`
    /// would have — so Round 2/3 circuits are oblivious to which side
    /// of the bridge the request came from.
    fn apply_bridge_trigger(
        env: &Env,
        event_id: &BytesN<20>,
        payload: BridgeTriggerPayload,
    ) -> Result<(), OracleError> {
        let id = storage::next_request_id(env);
        let now = env.ledger().timestamp();
        let info = RequestInfo {
            asset: payload.asset.clone(),
            range_secs: payload.range_secs,
            requested_at: now,
            origin: Some(payload.eth_origin.clone()),
        };
        storage::save_request(env, id, &info);
        storage::mark_event_seen(env, event_id);
        storage::extend_instance_ttl(env);

        env.events().publish(
            (symbol_short!("twapreq"), id),
            TwapRequestData {
                asset: payload.asset,
                range_secs: payload.range_secs,
                requested_at: now,
                originator: Some(payload.eth_origin),
            },
        );
        Ok(())
    }

    // ── reads ─────────────────────────────────────────────────────────

    pub fn verification_contract(env: Env) -> Address {
        storage::get_verification_contract(&env)
    }

    pub fn final_twap(env: Env, request_id: u64) -> Option<i128> {
        storage::get_final(&env, request_id)
    }

    pub fn latest(env: Env, asset: Symbol) -> Option<LatestTwap> {
        storage::get_latest(&env, &asset)
    }

    pub fn request(env: Env, request_id: u64) -> Option<RequestInfo> {
        storage::get_request(&env, request_id)
    }

    pub fn round2_bundle(env: Env, request_id: u64) -> Round2Bundle {
        storage::load_bundle(&env, request_id)
    }

    pub fn quorum(env: Env) -> (u32, u32) {
        (
            storage::get_quorum_numerator(&env),
            storage::get_quorum_denominator(&env),
        )
    }

    /// Required by `StellarHandlerInterface` — project-root uses this to
    /// recognise the contract as a handler.
    pub fn payload(env: Env, event_id: BytesN<20>) -> Option<Bytes> {
        let _ = (env, event_id);
        None
    }
}

// ─── event payload structs ────────────────────────────────────────────

/// Data field of the `TwapRequest` event. Topic shape is
/// `(Symbol("twapreq"), u64(request_id))`.
#[contracttype]
#[derive(Clone)]
pub struct TwapRequestData {
    pub asset: Symbol,
    pub range_secs: u32,
    pub requested_at: u64,
    /// `None` for Stellar-native requests; `Some(eth_address)` for
    /// requests bridged from Sepolia — the MetaMask `msg.sender` that
    /// fired the `TwapRequested` log. The frontend filters this field
    /// to associate Round 2/Final events with the wallet that asked.
    pub originator: Option<BytesN<20>>,
}

/// Data field of the `Round2Ready` composition event.
#[contracttype]
#[derive(Clone)]
pub struct Round2ReadyData {
    pub asset: Symbol,
    pub range_secs: u32,
    pub bundle: Round2Bundle,
}

#[contracttype]
#[derive(Clone)]
pub struct FinalizedData {
    pub asset: Symbol,
    pub median: i128,
    pub n_attestations: u32,
    pub computed_at: u64,
}

// ─── helpers ──────────────────────────────────────────────────────────

fn ceil_div(num: u32, denom: u32) -> u32 {
    debug_assert!(denom > 0);
    (num + denom - 1) / denom
}

/// Count the registered signers on the ed25519-security module the
/// verification contract points at. Goes through the verification module's
/// own client so the oracle never needs to know the security address.
fn security_signer_count(env: &Env, verification: &Address) -> u32 {
    use warpdrive_shared::interfaces::security::Ed25519SecurityClient;
    let security_addr =
        Ed25519VerificationClient::new(env, verification).security_contract();
    Ed25519SecurityClient::new(env, &security_addr)
        .list_signers()
        .len() as u32
}
