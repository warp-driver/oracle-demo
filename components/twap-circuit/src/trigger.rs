//! Decode the OracleContract `TwapRequest` Soroban event into the three
//! fields the rest of the circuit needs:
//!
//! - topic[0]: `Symbol("twapreq")`  — sanity check.
//! - topic[1]: `u64(request_id)`    — for ordering + payload.
//! - value:    `ScVal::Map`         — `TwapRequestData { asset, range_secs,
//!                                    requested_at }`. We only need
//!                                    `asset` and `range_secs`; the
//!                                    `requested_at` field is ignored
//!                                    here (the wall-clock at compute time
//!                                    is what goes into `Round2Payload`).
//!
//! Stellar events deliver topic strings + the `value` string as either
//! serde-tagged JSON (the engine's default) or XDR-base64 (older trigger
//! paths). We try JSON first because that's the documented shape for this
//! demo, then fall back to XDR-base64 — same dual-path the
//! `_helpers::trigger::parse_stellar_u64` reference uses.

use anyhow::{anyhow, Context, Result};
use stellar_xdr::curr::{Limits, ReadXdr, ScSymbol, ScVal};

pub struct TwapRequest {
    pub request_id: u64,
    /// The asset Symbol exactly as it appeared in the event — preserved
    /// byte-for-byte so the round-trip `info.asset == payload.asset`
    /// equality check on chain passes. Use `key_for(&asset)` to derive
    /// the KV-bucket key.
    pub asset: String,
    pub range_secs: u32,
}

pub fn parse_twap_request(topic_segments: &[String], value: &str) -> Result<TwapRequest> {
    if topic_segments.len() < 2 {
        return Err(anyhow!(
            "expected at least 2 topic segments, got {}",
            topic_segments.len()
        ));
    }

    let kind = parse_symbol(&topic_segments[0]).context("decode topic[0]")?;
    if kind != "twapreq" {
        return Err(anyhow!(
            "unexpected first topic symbol: {kind:?} (want \"twapreq\")"
        ));
    }
    let request_id = parse_u64(&topic_segments[1]).context("decode topic[1] (request_id)")?;

    let body = parse_scval(value).context("decode TwapRequest value")?;
    let entries = match body {
        ScVal::Map(Some(map)) => map.0.into_vec(),
        other => {
            return Err(anyhow!(
                "TwapRequest value is not a ScVal::Map: {other:?}"
            ))
        }
    };

    let mut asset: Option<String> = None;
    let mut range_secs: Option<u32> = None;
    for entry in entries {
        let key = match entry.key {
            ScVal::Symbol(ScSymbol(s)) => s.to_string(),
            other => return Err(anyhow!("non-symbol map key: {other:?}")),
        };
        match key.as_str() {
            "asset" => {
                asset = Some(match entry.val {
                    ScVal::Symbol(ScSymbol(s)) => s.to_string(),
                    ScVal::String(s) => s.to_string(),
                    other => {
                        return Err(anyhow!(
                            "TwapRequest.asset is not a Symbol/String: {other:?}"
                        ))
                    }
                });
            }
            "range_secs" => {
                range_secs = Some(match entry.val {
                    ScVal::U32(n) => n,
                    other => {
                        return Err(anyhow!(
                            "TwapRequest.range_secs is not a U32: {other:?}"
                        ))
                    }
                });
            }
            // `requested_at` is informational — we use the live wall-clock
            // when computing the TWAP window, not the request timestamp.
            _ => {}
        }
    }

    Ok(TwapRequest {
        request_id,
        asset: asset.ok_or_else(|| anyhow!("TwapRequest missing `asset` field"))?,
        range_secs: range_secs.ok_or_else(|| anyhow!("TwapRequest missing `range_secs` field"))?,
    })
}

/// Fold a contract-side asset symbol into the canonical KV-bucket key.
/// The contract emits `"btc_usd"` / `"eth_usd"` (Soroban Symbols can't
/// contain hyphens, only `[a-zA-Z0-9_]`); we also accept the short
/// `"btc"` / `"eth"` aliases as a courtesy and fold them into the same
/// canonical key the cron-circuit writes.
pub fn key_for(asset: &str) -> String {
    let lower = asset.to_ascii_lowercase();
    match lower.as_str() {
        "btc" => "btc_usd".to_string(),
        "eth" => "eth_usd".to_string(),
        _ => lower,
    }
}

fn parse_u64(raw: &str) -> Result<u64> {
    match parse_scval(raw)? {
        ScVal::U64(n) => Ok(n),
        other => Err(anyhow!("expected ScVal::U64, got {other:?}")),
    }
}

fn parse_symbol(raw: &str) -> Result<String> {
    match parse_scval(raw)? {
        ScVal::Symbol(ScSymbol(s)) => Ok(s.to_string()),
        other => Err(anyhow!("expected ScVal::Symbol, got {other:?}")),
    }
}

fn parse_scval(raw: &str) -> Result<ScVal> {
    if let Ok(v) = serde_json::from_str::<ScVal>(raw) {
        return Ok(v);
    }
    ScVal::from_xdr_base64(raw, Limits::none())
        .map_err(|e| anyhow!("ScVal neither valid JSON nor XDR-base64 ({raw:?}): {e:?}"))
}
