//! Persistent + instance storage for the Oracle contract.
//!
//! Storage is split deliberately:
//!
//! - **Instance** holds singleton config (`Admin`, `Version`, the address of
//!   the ed25519 verification module, the current quorum target). The
//!   instance footprint is tiny so we extend its TTL on every mutation.
//!
//! - **Persistent** holds *per-request* state. Each entry uses a uniquely-keyed
//!   `DataKey` variant so we never need a `Map<RequestId, T>` (which would
//!   pull the whole map into a single read on access — bounded keys keep
//!   reads O(1) even as the request set grows).
//!
//! - **Replay protection** uses the same per-`event_id` key pattern as the
//!   reference `stellar-handler`.

use soroban_sdk::{contracttype, Address, BytesN, Env, String, Symbol, Vec};
use warpdrive_shared::ttl;

use crate::contract::{LatestTwap, Round2Bundle};

#[contracttype]
pub enum DataKey {
    Admin,
    Version,
    VerificationContract,
    /// Auto-incremented; bumped on every `request_twap`.
    NextRequestId,
    /// 4/5 of total signer weight by default; admin can re-tune.
    QuorumNumerator,
    QuorumDenominator,

    /// Replay protection — set after a successful `verify_xlm`/`check_one`.
    EventSeen(BytesN<20>),

    /// Per-request metadata, written at `request_twap`.
    Request(u64),
    /// Per-request bundle of (signer, signature, twap) attestations.
    Attestations(u64),
    /// Has `Round2Ready` already been emitted for this request?
    Round2Released(u64),
    /// Final aggregated TWAP per request.
    FinalTwap(u64),
    /// Most-recent finalized TWAP per asset symbol (for cheap reads).
    Latest(Symbol),
}

#[contracttype]
#[derive(Clone)]
pub struct RequestInfo {
    pub asset: Symbol,
    pub range_secs: u32,
    pub requested_at: u64,
    /// `None` for native Stellar requests; `Some(eth_address)` for
    /// requests bridged in from an EVM chain (Sepolia in this demo).
    /// The address is the Sepolia `msg.sender` that fired the
    /// `TwapRequested` event the warpdrive bridge circuit observed.
    pub origin: Option<BytesN<20>>,
}

// ─── instance ─────────────────────────────────────────────────────────

pub fn set_verification_contract(env: &Env, addr: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::VerificationContract, addr);
}

pub fn get_verification_contract(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::VerificationContract)
        .expect("verification contract not set")
}

pub fn set_version(env: &Env, v: &String) {
    env.storage().instance().set(&DataKey::Version, v);
}

pub fn set_quorum(env: &Env, num: u32, denom: u32) {
    env.storage().instance().set(&DataKey::QuorumNumerator, &num);
    env.storage()
        .instance()
        .set(&DataKey::QuorumDenominator, &denom);
}

pub fn get_quorum_numerator(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::QuorumNumerator)
        .unwrap_or(4)
}

pub fn get_quorum_denominator(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::QuorumDenominator)
        .unwrap_or(5)
}

pub fn next_request_id(env: &Env) -> u64 {
    let key = DataKey::NextRequestId;
    let id: u64 = env.storage().instance().get(&key).unwrap_or(0);
    env.storage().instance().set(&key, &(id + 1));
    id
}

pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(ttl::INSTANCE_RENEWAL_THRESHOLD, ttl::INSTANCE_TARGET_TTL);
}

// ─── replay protection ────────────────────────────────────────────────

pub fn is_event_seen(env: &Env, event_id: &BytesN<20>) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::EventSeen(event_id.clone()))
}

pub fn mark_event_seen(env: &Env, event_id: &BytesN<20>) {
    let key = DataKey::EventSeen(event_id.clone());
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(
        &key,
        ttl::PERSISTENT_RENEWAL_THRESHOLD,
        ttl::PERSISTENT_TARGET_TTL,
    );
}

// ─── requests ─────────────────────────────────────────────────────────

pub fn save_request(env: &Env, id: u64, info: &RequestInfo) {
    let key = DataKey::Request(id);
    env.storage().persistent().set(&key, info);
    env.storage().persistent().extend_ttl(
        &key,
        ttl::PERSISTENT_RENEWAL_THRESHOLD,
        ttl::PERSISTENT_TARGET_TTL,
    );
}

pub fn get_request(env: &Env, id: u64) -> Option<RequestInfo> {
    env.storage().persistent().get(&DataKey::Request(id))
}

// ─── round 2 attestation bundle ───────────────────────────────────────

pub fn load_bundle(env: &Env, id: u64) -> Round2Bundle {
    env.storage()
        .persistent()
        .get(&DataKey::Attestations(id))
        .unwrap_or_else(|| Round2Bundle {
            attestations: Vec::new(env),
        })
}

pub fn save_bundle(env: &Env, id: u64, bundle: &Round2Bundle) {
    let key = DataKey::Attestations(id);
    env.storage().persistent().set(&key, bundle);
    env.storage().persistent().extend_ttl(
        &key,
        ttl::PERSISTENT_RENEWAL_THRESHOLD,
        ttl::PERSISTENT_TARGET_TTL,
    );
}

pub fn round2_released(env: &Env, id: u64) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Round2Released(id))
        .unwrap_or(false)
}

pub fn mark_round2_released(env: &Env, id: u64) {
    let key = DataKey::Round2Released(id);
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(
        &key,
        ttl::PERSISTENT_RENEWAL_THRESHOLD,
        ttl::PERSISTENT_TARGET_TTL,
    );
}

// ─── final results ────────────────────────────────────────────────────

pub fn save_final(env: &Env, id: u64, asset: &Symbol, median: i128, ts: u64) {
    let key = DataKey::FinalTwap(id);
    env.storage().persistent().set(&key, &median);
    env.storage().persistent().extend_ttl(
        &key,
        ttl::PERSISTENT_RENEWAL_THRESHOLD,
        ttl::PERSISTENT_TARGET_TTL,
    );
    let latest_key = DataKey::Latest(asset.clone());
    env.storage()
        .persistent()
        .set(&latest_key, &LatestTwap { twap: median, ts });
    env.storage().persistent().extend_ttl(
        &latest_key,
        ttl::PERSISTENT_RENEWAL_THRESHOLD,
        ttl::PERSISTENT_TARGET_TTL,
    );
}

pub fn get_final(env: &Env, id: u64) -> Option<i128> {
    env.storage().persistent().get(&DataKey::FinalTwap(id))
}

pub fn get_latest(env: &Env, asset: &Symbol) -> Option<LatestTwap> {
    env.storage()
        .persistent()
        .get(&DataKey::Latest(asset.clone()))
}
