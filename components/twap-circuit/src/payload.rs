//! XDR-encode `SubmissionPayload::Round2(Round2Payload)` so the on-chain
//! `OracleContract::verify_xlm` decodes the bytes back and dispatches.
//!
//! Soroban serialises a `#[contracttype]` tuple-variant enum as
//! `ScVal::Vec(Some([ScVal::Symbol("VariantName"), <inner>]))`. The
//! inner Round2Payload struct is itself an `ScVal::Map(Vec<ScMapEntry>)`
//! with entries sorted by key (`ScVal::Symbol` of the field name) in
//! ascending byte order — alphabetic for ASCII names.
//!
//! Mis-ordering either layer breaks the on-chain decode silently
//! (you get `InvalidEnvelope`), so this module is the single source of
//! truth for the wire format.
//!
//! Round2Payload field order (alphabetic):
//!     asset, computed_at, range_secs, request_id, twap.

use anyhow::{Context, Result};
use stellar_xdr::curr::{
    Int128Parts, Limits, ScMap, ScMapEntry, ScSymbol, ScVal, ScVec, StringM, VecM, WriteXdr,
};

pub fn encode_round2(
    asset: &str,
    request_id: u64,
    range_secs: u32,
    twap_e7: i128,
    computed_at: u64,
) -> Result<Vec<u8>> {
    // Split a Rust `i128` into the (hi: i64, lo: u64) shape Soroban's
    // `Int128Parts` uses. Same trick as hodlers-app's circuit/payload.rs.
    let hi = (twap_e7 >> 64) as i64;
    let lo = (twap_e7 as u128 & u64::MAX as u128) as u64;

    let asset_sym: StringM<32> = asset
        .as_bytes()
        .try_into()
        .context("asset symbol too long for ScSymbol (≤32 bytes)")?;

    // Inner struct: Round2Payload as ScVal::Map, fields sorted alphabetically.
    let entries = vec![
        ScMapEntry {
            key: symbol_val("asset")?,
            val: ScVal::Symbol(ScSymbol(asset_sym)),
        },
        ScMapEntry {
            key: symbol_val("computed_at")?,
            val: ScVal::U64(computed_at),
        },
        ScMapEntry {
            key: symbol_val("range_secs")?,
            val: ScVal::U32(range_secs),
        },
        ScMapEntry {
            key: symbol_val("request_id")?,
            val: ScVal::U64(request_id),
        },
        ScMapEntry {
            key: symbol_val("twap")?,
            val: ScVal::I128(Int128Parts { hi, lo }),
        },
    ];
    let map = ScMap(
        entries
            .try_into()
            .context("ScMap construction (entry count > vec capacity)")?,
    );
    let inner = ScVal::Map(Some(map));

    // Wrap in the SubmissionPayload::Round2 variant.
    let variant: VecM<ScVal> = vec![symbol_val("Round2")?, inner]
        .try_into()
        .context("ScVec construction")?;
    ScVal::Vec(Some(ScVec(variant)))
        .to_xdr(Limits::none())
        .context("xdr-encode SubmissionPayload::Round2")
}

fn symbol_val(s: &str) -> Result<ScVal> {
    let inner: StringM<32> = s
        .as_bytes()
        .try_into()
        .context("field-name symbol too long for ScSymbol (≤32 bytes)")?;
    Ok(ScVal::Symbol(ScSymbol(inner)))
}
