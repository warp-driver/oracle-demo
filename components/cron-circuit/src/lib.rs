//! Cron circuit — Round 1 of the BTC/ETH price oracle.
//!
//! Trigger: cron. The schedule (e.g. every 10 min) is set externally in
//! `service.json`; the only field this code sees is `trigger_time`.
//!
//! Submit kind: `Submit::None`. Every successful tick returns `Ok(vec![])`
//! — round-1 only writes to KV, it never produces a signed envelope.
//!
//! Network: this circuit ONLY contacts `api.coingecko.com`. The host
//! capability `allowed_http_hosts = ["api.coingecko.com"]` MUST be
//! declared in the workflow's `service.json`; without it the engine
//! blocks the outbound request and `run_inner` returns `Err`.
//!
//! CoinGecko endpoint:
//!     `GET https://api.coingecko.com/api/v3/simple/price`
//!     `   ?ids=bitcoin,ethereum`
//!     `   &vs_currencies=usd`
//!     `   &include_last_updated_at=true`
//!
//! Fields read from the JSON response, per coin:
//!   * `<coin>.usd`             — f64, spot price in USD
//!   * `<coin>.last_updated_at` — u64, unix-seconds source timestamp
//!
//! Persistence: appends `Sample { ts, price_e7 }` (bincode 1.3, positional
//! `u64 || i128` little-endian) to bucket `"oracle-cron-samples"` under
//! keys `"samples/btc-usd"` / `"samples/eth-usd"`. Each tick prunes any
//! sample older than 25 h relative to the freshest entry so the twap
//! circuit can serve any window up to 24 h with one extra hour of slack.

mod coingecko;
mod store;
mod types;

wit_bindgen::generate!({
    world: "circuit-world",
    path: "../../wit-definitions/wit",
    generate_all,
});

use warpdrive::vectr::input::TriggerData;

struct Component;

impl Guest for Component {
    fn run(trigger_action: TriggerAction) -> Result<Vec<WasmResponse>, String> {
        run_inner(trigger_action).map_err(|e| format!("cron-circuit: {e:#}"))
    }
}

fn run_inner(trigger_action: TriggerAction) -> anyhow::Result<Vec<WasmResponse>> {
    match trigger_action.data {
        TriggerData::Cron(_) => {}
        _ => anyhow::bail!("expected Cron trigger"),
    }

    for (asset, ts, price_e7) in coingecko::fetch_now()? {
        store::append(asset, ts, price_e7)?;
    }

    Ok(vec![])
}

export!(Component);
