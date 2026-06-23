use anyhow::{anyhow, Context, Result};

use crate::types::Sample;
use crate::wasi::keyvalue::{atomics, store};

/// Shared bucket name — the twap-circuit reads back from the same one.
const BUCKET: &str = "oracle-cron-samples";

/// Rolling-window retention. 25 h = 24 h longest TWAP range + 1 h slack
/// so the boundary sample doesn't get pruned right before a request lands.
const WINDOW_SECS: u64 = 25 * 3600;

/// Append a new `Sample { ts, price_e7 }` to the per-asset key and prune
/// anything older than 25 h relative to the freshest sample currently in
/// the bucket. Uses `wasi:keyvalue/atomics` CAS so overlapping cron firings
/// compose without dropping each other's writes.
///
/// `asset` is `"btc_usd"` or `"eth_usd"` (Soroban Symbol charset); the
/// storage key is `samples/<asset>` and is read verbatim by the twap-circuit.
pub fn append(asset: &str, ts: u64, price_e7: i128) -> Result<()> {
    let bucket = store::open(&BUCKET.to_string())
        .map_err(|e| anyhow!("open kv bucket `{BUCKET}`: {e:?}"))?;
    let key = format!("samples/{asset}");

    loop {
        let cas = atomics::Cas::new(&bucket, &key)
            .map_err(|e| anyhow!("cas open `{key}`: {e:?}"))?;

        let mut samples: Vec<Sample> = match cas
            .current()
            .map_err(|e| anyhow!("cas current `{key}`: {e:?}"))?
        {
            Some(bytes) => bincode::deserialize(&bytes)
                .with_context(|| format!("deserialize samples at `{key}`"))?,
            None => Vec::new(),
        };

        samples.push(Sample { ts, price_e7 });

        // Use the freshest ts as the anchor so a stale incoming `ts`
        // (clock skew on CoinGecko's side) never prunes newer entries.
        let max_ts = samples.iter().map(|s| s.ts).max().unwrap_or(ts);
        let cutoff = max_ts.saturating_sub(WINDOW_SECS);
        samples.retain(|s| s.ts >= cutoff);

        let bytes = bincode::serialize(&samples)
            .with_context(|| format!("serialize samples at `{key}`"))?;

        match atomics::swap(cas, &bytes) {
            Ok(()) => return Ok(()),
            Err(atomics::CasError::CasFailed(_)) => continue,
            Err(atomics::CasError::StoreError(e)) => {
                return Err(anyhow!("cas swap `{key}`: {e:?}"))
            }
        }
    }
}
