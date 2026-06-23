//! XDR encoder for `FinalPayload` — the bytes the Round 3 circuit feeds
//! into the host-signed `XlmEnvelope` for `OracleContract::submit_final`.
//!
//! Soroban serializes a `#[contracttype] struct` as `ScVal::Map` with
//! entries sorted by key (Symbol of the field name) in ascending byte
//! order, which for these ASCII identifiers is plain alphabetic. The on-
//! chain decoder will reject the payload if entry order or any field type
//! differs from the contract definition. Field order here MUST match
//! `oracle::FinalPayload`:
//!   asset, computed_at, median, n_attestations, request_id.

use anyhow::{Context, Result};
use stellar_xdr::curr::{
    Int128Parts, Limits, ScMap, ScMapEntry, ScSymbol, ScVal, StringM, WriteXdr,
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

    let entries = vec![
        ScMapEntry {
            key: symbol_val("asset")?,
            val: symbol_val(asset)?,
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
    ScVal::Map(Some(map))
        .to_xdr(Limits::none())
        .context("xdr-encode FinalPayload")
}

fn symbol_val(s: &str) -> Result<ScVal> {
    let inner: StringM<32> = s.as_bytes().try_into().context("symbol too long")?;
    Ok(ScVal::Symbol(ScSymbol(inner)))
}
