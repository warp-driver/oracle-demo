//! Decode a Soroban `Round2Ready` event into the typed bundle the median
//! circuit operates on.
//!
//! Event shape (set by `OracleContract::submit_round2` once the per-request
//! attestation set crosses quorum):
//!   topic 0: `ScVal::Symbol("r2ready")`
//!   topic 1: `ScVal::U64(request_id)`
//!   value:   `ScVal::Map(Round2ReadyData)` — fields in alphabetic order:
//!            `asset: Symbol`, `bundle: Round2Bundle { attestations: Vec<Round2Attestation> }`,
//!            `range_secs: U32`. Each `Round2Attestation` is itself a
//!            `ScVal::Map` with alphabetic fields:
//!            `computed_at: U64`, `envelope: Bytes`, `signature: BytesN<64>`,
//!            `signer: BytesN<32>`, `twap: I128`.
//!
//! WarpDrive's Stellar event poller forwards `topic_segments` and `value`
//! as opaque strings. The default `xdrFormat` for `getEvents` is
//! `base64`, but the engine sometimes pre-decodes to a `serde_json` form;
//! [`parse_scval`] tries JSON first, then XDR base64, so the circuit works
//! against either flavour without runtime configuration.

use anyhow::{anyhow, bail, Context};
use stellar_xdr::curr::{Int128Parts, Limits, ReadXdr, ScMapEntry, ScSymbol, ScVal};

use crate::warpdrive::types::chain::StellarEvent;

/// One Round 2 attestation, decoded from the on-chain `Round2Attestation`
/// struct carried inside the `Round2Ready` event bundle. Field types are
/// the native ones the median circuit needs — fixed-size key/signature
/// arrays for ed25519, `Vec<u8>` for the SEP-0053 envelope bytes.
pub struct DecodedAttestation {
    pub signer: [u8; 32],
    pub signature: [u8; 64],
    pub envelope: Vec<u8>,
    pub twap: i128,
    pub computed_at: u64,
}

/// Decoded `Round2Ready` event payload, plus the `request_id` extracted
/// from topic 1 so the caller can build a stable ordering / salt without
/// re-parsing the topics.
pub struct DecodedBundle {
    pub request_id: u64,
    pub asset: String,
    pub range_secs: u32,
    pub attestations: Vec<DecodedAttestation>,
}

pub fn parse_round2_ready(event: &StellarEvent) -> anyhow::Result<DecodedBundle> {
    if event.topic_segments.len() < 2 {
        bail!(
            "Round2Ready event needs >=2 topics, got {}",
            event.topic_segments.len()
        );
    }

    let topic0 = parse_scval(&event.topic_segments[0]).context("decode topic 0")?;
    let symbol = match topic0 {
        ScVal::Symbol(ScSymbol(s)) => String::from_utf8(s.to_vec())?,
        other => bail!("topic 0 not a Symbol: {other:?}"),
    };
    if symbol != "r2ready" {
        bail!("expected topic 0 == Symbol(\"r2ready\"), got {symbol:?}");
    }

    let topic1 = parse_scval(&event.topic_segments[1]).context("decode topic 1")?;
    let request_id = match topic1 {
        ScVal::U64(n) => n,
        other => bail!("topic 1 not a U64: {other:?}"),
    };

    let value = parse_scval(&event.value).context("decode event value")?;
    let entries = expect_map(&value).context("event.value not a Map")?;

    let asset = expect_symbol(get_field(entries, "asset")?)?;
    let range_secs = expect_u32(get_field(entries, "range_secs")?)?;

    let bundle_entries = expect_map(get_field(entries, "bundle")?)?;
    let attestations_vec = expect_vec(get_field(bundle_entries, "attestations")?)?;
    let attestations = attestations_vec
        .iter()
        .map(decode_attestation)
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(DecodedBundle {
        request_id,
        asset,
        range_secs,
        attestations,
    })
}

fn decode_attestation(val: &ScVal) -> anyhow::Result<DecodedAttestation> {
    let entries = expect_map(val).context("attestation not a Map")?;
    let computed_at = expect_u64(get_field(entries, "computed_at")?)?;
    let envelope = expect_bytes(get_field(entries, "envelope")?)?;
    let signature_bytes = expect_bytes(get_field(entries, "signature")?)?;
    let signer_bytes = expect_bytes(get_field(entries, "signer")?)?;
    let twap = expect_i128(get_field(entries, "twap")?)?;

    let signer: [u8; 32] = signer_bytes
        .try_into()
        .map_err(|v: Vec<u8>| anyhow!("signer not 32 bytes (got {})", v.len()))?;
    let signature: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|v: Vec<u8>| anyhow!("signature not 64 bytes (got {})", v.len()))?;

    Ok(DecodedAttestation {
        signer,
        signature,
        envelope,
        twap,
        computed_at,
    })
}

// ── ScVal helpers ─────────────────────────────────────────────────────

/// Decode a string-encoded `ScVal` the WarpDrive engine handed us on a
/// `StellarEvent` field. The host sometimes emits the JSON form (the
/// stellar-xdr `serde` representation) and sometimes the XDR-base64 form
/// (Stellar RPC default). Try JSON first so we keep the typed value path,
/// then fall back to XDR base64.
fn parse_scval(raw: &str) -> anyhow::Result<ScVal> {
    if let Ok(v) = serde_json::from_str::<ScVal>(raw) {
        return Ok(v);
    }
    ScVal::from_xdr_base64(raw, Limits::none())
        .map_err(|e| anyhow!("ScVal decode (neither JSON nor XDR-base64): {e}"))
}

fn get_field<'a>(entries: &'a [ScMapEntry], field: &str) -> anyhow::Result<&'a ScVal> {
    entries
        .iter()
        .find(|e| {
            matches!(
                &e.key,
                ScVal::Symbol(ScSymbol(s)) if s.as_slice() == field.as_bytes()
            )
        })
        .map(|e| &e.val)
        .ok_or_else(|| anyhow!("missing field {field:?} in ScMap"))
}

fn expect_map(val: &ScVal) -> anyhow::Result<&[ScMapEntry]> {
    match val {
        ScVal::Map(Some(m)) => Ok(m.0.as_slice()),
        other => bail!("expected ScVal::Map, got {other:?}"),
    }
}

fn expect_vec(val: &ScVal) -> anyhow::Result<&[ScVal]> {
    match val {
        ScVal::Vec(Some(v)) => Ok(v.0.as_slice()),
        other => bail!("expected ScVal::Vec, got {other:?}"),
    }
}

fn expect_symbol(val: &ScVal) -> anyhow::Result<String> {
    match val {
        ScVal::Symbol(ScSymbol(s)) => Ok(String::from_utf8(s.to_vec())?),
        other => bail!("expected ScVal::Symbol, got {other:?}"),
    }
}

fn expect_u64(val: &ScVal) -> anyhow::Result<u64> {
    match val {
        ScVal::U64(n) => Ok(*n),
        other => bail!("expected ScVal::U64, got {other:?}"),
    }
}

fn expect_u32(val: &ScVal) -> anyhow::Result<u32> {
    match val {
        ScVal::U32(n) => Ok(*n),
        other => bail!("expected ScVal::U32, got {other:?}"),
    }
}

fn expect_bytes(val: &ScVal) -> anyhow::Result<Vec<u8>> {
    match val {
        ScVal::Bytes(b) => Ok(b.0.to_vec()),
        other => bail!("expected ScVal::Bytes, got {other:?}"),
    }
}

fn expect_i128(val: &ScVal) -> anyhow::Result<i128> {
    match val {
        ScVal::I128(Int128Parts { hi, lo }) => {
            Ok(((*hi as i128) << 64) | (*lo as u128 as i128))
        }
        other => bail!("expected ScVal::I128, got {other:?}"),
    }
}
