//! XDR encoder for `SubmissionPayload::BridgeTrigger(BridgeTriggerPayload)`
//! — the bytes the aggregator places into the `XlmEnvelope.payload`
//! field before calling `OracleContract::verify_xlm`.
//!
//! Soroban serialises a `#[contracttype]` tuple-variant enum as
//! `ScVal::Vec(Some([ScVal::Symbol("VariantName"), <inner>]))`. The
//! inner `BridgeTriggerPayload` struct is itself a `ScVal::Map` whose
//! entries MUST be sorted by key (the field-name `Symbol`) in ascending
//! byte order — for these ASCII identifiers, plain alphabetic order.
//!
//! Alphabetic order for the locked struct fields:
//!   asset, block_timestamp, eth_origin, eth_tx_hash, range_secs.
//!
//! Mis-ordering produces XDR that decodes to a structurally valid
//! `ScVal::Map` on the contract side but fails the `#[contracttype]`
//! field-by-field check, returning `InvalidEnvelope` with no further
//! diagnostic. So: keep this order in lock-step with
//! `contracts/oracle/src/payload.rs`.

use anyhow::{Context, Result};
use stellar_xdr::curr::{
    BytesM, Limits, ScBytes, ScMap, ScMapEntry, ScSymbol, ScVal, ScVec, StringM, VecM, WriteXdr,
};

use crate::trigger::DecodedEvent;

pub fn encode_bridge(d: &DecodedEvent) -> Result<Vec<u8>> {
    let asset_sym: StringM<32> = d
        .asset
        .as_bytes()
        .try_into()
        .context("asset symbol too long for ScSymbol (≤32 bytes)")?;

    let entries = vec![
        ScMapEntry {
            key: symbol_val("asset")?,
            val: ScVal::Symbol(ScSymbol(asset_sym)),
        },
        ScMapEntry {
            key: symbol_val("block_timestamp")?,
            val: ScVal::U64(d.block_timestamp),
        },
        ScMapEntry {
            key: symbol_val("eth_origin")?,
            val: bytes_val(&d.eth_origin)?,
        },
        ScMapEntry {
            key: symbol_val("eth_tx_hash")?,
            val: bytes_val(&d.eth_tx_hash)?,
        },
        ScMapEntry {
            key: symbol_val("range_secs")?,
            val: ScVal::U32(d.range_secs),
        },
    ];
    let map = ScMap(entries.try_into().context("ScMap construction")?);
    let inner = ScVal::Map(Some(map));

    // Wrap in the `SubmissionPayload::BridgeTrigger` tuple variant.
    let variant: VecM<ScVal> = vec![symbol_val("BridgeTrigger")?, inner]
        .try_into()
        .context("ScVec construction")?;
    ScVal::Vec(Some(ScVec(variant)))
        .to_xdr(Limits::none())
        .context("xdr-encode SubmissionPayload::BridgeTrigger")
}

fn symbol_val(s: &str) -> Result<ScVal> {
    let inner: StringM<32> = s.as_bytes().try_into().context("symbol too long")?;
    Ok(ScVal::Symbol(ScSymbol(inner)))
}

/// Build a `ScVal::Bytes` from a fixed-size byte slice. Soroban
/// `BytesN<N>` decodes a `ScVal::Bytes` of length N; the on-chain
/// validator enforces N, so we just emit the bytes verbatim.
fn bytes_val(bytes: &[u8]) -> Result<ScVal> {
    let inner: BytesM = bytes
        .to_vec()
        .try_into()
        .context("bytes too long for ScBytes")?;
    Ok(ScVal::Bytes(ScBytes(inner)))
}
