//! XDR encoder for `SubmissionPayload::Final(FinalPayload)` — the bytes
//! the Round 3 circuit feeds into the host-signed `XlmEnvelope` for
//! `OracleContract::verify_xlm`.
//!
//! Soroban serialises a `#[contracttype]` tuple-variant enum as
//! `ScVal::Vec(Some([ScVal::Symbol("VariantName"), <inner>]))`. The
//! inner FinalPayload struct is itself an `ScVal::Map` with entries
//! sorted by key (Symbol of the field name) in ascending byte order,
//! which for these ASCII identifiers is plain alphabetic.
//!
//! FinalPayload field order MUST match `oracle::FinalPayload`:
//!   asset, computed_at, median, n_attestations, request_id.

use anyhow::{Context, Result};
use stellar_xdr::curr::{
    Int128Parts, Limits, ScMap, ScMapEntry, ScSymbol, ScVal, ScVec, StringM, VecM, WriteXdr,
};

pub fn encode_final(
    asset: &str,
    request_id: u64,
    median: i128,
    n_attestations: u32,
    computed_at: u64,
) -> Result<Vec<u8>> {
    let hi = (median >> 64) as i64;
    let lo = (median as u128 & u64::MAX as u128) as u64;

    let asset_sym: StringM<32> = asset
        .as_bytes()
        .try_into()
        .context("asset symbol too long for ScSymbol (≤32 bytes)")?;

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
            key: symbol_val("median")?,
            val: ScVal::I128(Int128Parts { hi, lo }),
        },
        ScMapEntry {
            key: symbol_val("n_attestations")?,
            val: ScVal::U32(n_attestations),
        },
        ScMapEntry {
            key: symbol_val("request_id")?,
            val: ScVal::U64(request_id),
        },
    ];
    let map = ScMap(entries.try_into().context("ScMap construction")?);
    let inner = ScVal::Map(Some(map));

    // Wrap in the SubmissionPayload::Final variant.
    let variant: VecM<ScVal> = vec![symbol_val("Final")?, inner]
        .try_into()
        .context("ScVec construction")?;
    ScVal::Vec(Some(ScVec(variant)))
        .to_xdr(Limits::none())
        .context("xdr-encode SubmissionPayload::Final")
}

fn symbol_val(s: &str) -> Result<ScVal> {
    let inner: StringM<32> = s.as_bytes().try_into().context("symbol too long")?;
    Ok(ScVal::Symbol(ScSymbol(inner)))
}
