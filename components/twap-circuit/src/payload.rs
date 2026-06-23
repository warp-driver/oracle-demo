//! XDR-encode `Round2Payload` so `OracleContract::submit_round2` can
//! `Round2Payload::from_xdr` the bytes back.
//!
//! Soroban serializes a `#[contracttype]` struct as
//! `ScVal::Map(Vec<ScMapEntry>)` with entries sorted by `key`
//! (`ScVal::Symbol` of the field name) in ascending byte order — which
//! for simple ASCII names is alphabetic. Mis-ordering breaks the on-chain
//! decode silently (you get `InvalidRound2Payload`), so this module
//! is the single source of truth for the layout.
//!
//! Field order (alphabetic):
//!     asset, computed_at, range_secs, request_id, twap.

use anyhow::{Context, Result};
use stellar_xdr::curr::{
    Int128Parts, Limits, ScMap, ScMapEntry, ScSymbol, ScVal, StringM, WriteXdr,
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

    // Entries MUST stay in alphabetic order — the on-chain decoder is
    // sort-sensitive. Re-ordering here would only surface as an opaque
    // `InvalidRound2Payload` error during testnet integration, so we
    // keep the layout visually obvious.
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

    ScVal::Map(Some(map))
        .to_xdr(Limits::none())
        .context("xdr-encode Round2Payload")
}

fn symbol_val(s: &str) -> Result<ScVal> {
    let inner: StringM<32> = s
        .as_bytes()
        .try_into()
        .context("field-name symbol too long for ScSymbol (≤32 bytes)")?;
    Ok(ScVal::Symbol(ScSymbol(inner)))
}
