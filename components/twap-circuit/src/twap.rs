//! Geometric TWAP over the cron-circuit's stored samples.
//!
//! ## Data source
//!
//! We read `Vec<Sample>` from the wasi:keyvalue bucket
//! `"oracle-cron-samples"` at key `"samples/<asset>"` — the exact shape
//! the cron-circuit writes (`bincode::serialize` of a `Vec<Sample>`,
//! `Sample { ts: u64 unix-seconds, price_e7: i128 (price * 1e7) }`).
//! bincode 1.3 is positional, so as long as field types + order match
//! between the producer (cron) and consumer (here), the wire shape is
//! stable without a shared crate.
//!
//! ## Geometric mean (the right average for prices)
//!
//! The geometric mean is the unique average that is invariant under
//! reciprocation (a one-pair `USD→BTC` then `BTC→USD` round-trip lands
//! back at the original price). It equals the exponential of the
//! arithmetic mean of log-prices:
//!
//! ```text
//!     log_mean  =  (1/n)  *  Σ ln(p_i / 1e7)
//!     twap_f64  =  exp(log_mean)              // back in dollar units
//!     twap_e7   =  round(twap_f64 * 1e7)      // fixed-point i128 for the chain
//! ```
//!
//! Working in log space side-steps i128 overflow when multiplying many
//! large prices and gives the same result as the equal-weighted n-th
//! root of the product up to floating-point rounding.
//!
//! ## Failure mode
//!
//! Returns `Err("no samples in range …")` when zero stored samples fall
//! in `[now - range_secs, now]`. We deliberately fail loud rather than
//! attesting to a fabricated zero — the host logs, and the aggregator
//! simply has nothing to gather for this Vectr, which is preferable to
//! submitting a bogus on-chain value.

use anyhow::{anyhow, Result};
use libm::{exp, log};
use serde::{Deserialize, Serialize};

use crate::trigger::key_for;
use crate::wasi::keyvalue::store;

const BUCKET: &str = "oracle-cron-samples";

/// 10^7 — the fixed-point scale every on-chain `i128` price uses. Held
/// as `f64` because all the math here is log/exp.
const PRICE_SCALE: f64 = 10_000_000.0;

/// Mirrors cron-circuit's `store::Sample`. bincode 1.3 is positional;
/// keeping field types + declaration order identical to the writer is
/// what makes the wire format compatible across crates.
#[derive(Serialize, Deserialize)]
struct Sample {
    ts: u64,
    price_e7: i128,
}

pub fn geometric_twap(asset: &str, range_secs: u32, now: u64) -> Result<i128> {
    let samples = load_samples(asset)?;
    let window_start = now.saturating_sub(range_secs as u64);

    let mut log_sum = 0.0_f64;
    let mut n = 0_u32;
    for s in &samples {
        if s.ts < window_start || s.ts > now {
            continue;
        }
        if s.price_e7 <= 0 {
            // `log(<=0)` is NaN; drop degenerate samples rather than
            // poison the entire mean with a single bad row.
            continue;
        }
        let price = (s.price_e7 as f64) / PRICE_SCALE;
        log_sum += log(price);
        n += 1;
    }

    if n == 0 {
        return Err(anyhow!(
            "no samples in range for asset={asset} window=[{window_start}, {now}] \
             (have {} sample(s) total in bucket)",
            samples.len()
        ));
    }

    let log_mean = log_sum / (n as f64);
    let twap_f64 = exp(log_mean);
    Ok((twap_f64 * PRICE_SCALE).round() as i128)
}

fn load_samples(asset: &str) -> Result<Vec<Sample>> {
    let bucket = store::open(&BUCKET.to_string())
        .map_err(|e| anyhow!("open kv bucket {BUCKET}: {e:?}"))?;
    let key = format!("samples/{}", key_for(asset));
    let bytes = bucket
        .get(&key)
        .map_err(|e| anyhow!("kv get {key}: {e:?}"))?
        .ok_or_else(|| anyhow!("no samples stored at key {key}"))?;
    bincode::deserialize::<Vec<Sample>>(&bytes)
        .map_err(|e| anyhow!("decode Vec<Sample> from {key}: {e:?}"))
}
