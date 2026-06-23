//! twap-circuit — Round 2 of the BTC/ETH multi-round price oracle.
//!
//! Triggered by the OracleContract's `TwapRequest` Soroban event:
//!   topic 0 = Symbol("twapreq"), topic 1 = u64(request_id),
//!   value   = TwapRequestData { asset, range_secs, requested_at }.
//!
//! Reads the rolling sample bucket the cron-circuit populates, computes a
//! geometric TWAP over samples whose `ts` falls in `[now - range_secs, now]`,
//! and returns ONE `WasmResponse` carrying the XDR-encoded `Round2Payload`
//! for the aggregator/host to envelope, sign, and submit via
//! `OracleContract::submit_round2`.

mod payload;
mod trigger;
mod twap;

wit_bindgen::generate!({
    world: "circuit-world",
    path: "../../wit-definitions/wit",
    generate_all,
});

use warpdrive::vectr::input::TriggerData;

/// Canonical KV-key form of the assets this oracle quotes. The
/// cron-circuit writes samples under `samples/<asset>` using these
/// strings, and the median-circuit consumes the same alphabet. The
/// `trigger::key_for` helper folds short variants ("btc",
/// "eth") into these canonical keys before any bucket lookup.
pub const ASSETS: &[&str] = &["btc_usd", "eth_usd"];

struct Component;

impl Guest for Component {
    fn run(t: TriggerAction) -> Result<Vec<WasmResponse>, String> {
        run_inner(t).map_err(|e| format!("twap-circuit: {e:#}"))
    }
}

fn run_inner(trigger_action: TriggerAction) -> anyhow::Result<Vec<WasmResponse>> {
    let event = match trigger_action.data {
        TriggerData::StellarContractEvent(e) => e.event,
        _ => anyhow::bail!("expected StellarContractEvent trigger"),
    };

    let req = trigger::parse_twap_request(&event.topic_segments, &event.value)?;
    let now = host_now_secs();
    let twap_e7 = twap::geometric_twap(&req.asset, req.range_secs, now)?;

    // The on-chain `submit_round2` validates `info.asset == payload.asset`
    // (contract.rs:253), so we MUST echo the asset Symbol byte-for-byte
    // — never the normalized KV key. Only `twap::geometric_twap` folds
    // short forms when picking a bucket key.
    let payload_bytes =
        payload::encode_round2(&req.asset, req.request_id, req.range_secs, twap_e7, now)?;

    // Unique-per-Vectr salt so each Vectr's event_id differs. Round 2 is
    // single-signer (every Vectr sees a slightly different CoinGecko
    // snapshot) and the contract dedups by `(request_id, signer)`, so
    // quorum-collapsing identical event_ids would discard valid
    // attestations. The payload bytes already differ across Vectrs —
    // reusing them as the salt is the cheapest unique fingerprint.
    let salt = payload_bytes.clone();
    Ok(vec![WasmResponse {
        payload: payload_bytes,
        ordering: Some(req.request_id),
        event_id_salt: Some(salt),
    }])
}

/// Unix-seconds wall-clock from the WASI host. `std::time::SystemTime`
/// is unreliable inside WASI components — `wasi:clocks/wall-clock` is
/// the supported source.
fn host_now_secs() -> u64 {
    crate::wasi::clocks::wall_clock::now().seconds
}

export!(Component);
