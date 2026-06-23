use anyhow::{anyhow, Context, Result};
use wstd::http::{Body, Client, Request};
use wstd::runtime::block_on;

/// CoinGecko simple-price endpoint. The host's HTTP allowlist (set in
/// service.json as `allowed_http_hosts = ["api.coingecko.com"]`) MUST
/// authorise this exact host or the request is rejected before it leaves
/// the sandbox.
const URL: &str = "https://api.coingecko.com/api/v3/simple/price\
?ids=bitcoin,ethereum&vs_currencies=usd&include_last_updated_at=true";

/// Fetch the latest BTC/USD and ETH/USD spot prices.
///
/// Returns one `(label, ts, price_e7)` tuple per asset — BTC first,
/// then ETH. Errors propagate; the caller surfaces them through
/// `run_inner` so node logs show the underlying HTTP / parse failure.
pub fn fetch_now() -> Result<Vec<(&'static str, u64, i128)>> {
    let body: serde_json::Value = block_on(async { fetch_json(URL).await })
        .context("coingecko GET failed")?;
    Ok(vec![
        extract(&body, "bitcoin", "btc_usd")?,
        extract(&body, "ethereum", "eth_usd")?,
    ])
}

/// Inlined helper — equivalent to `warpdrive-wasi-utils`' `fetch_json`,
/// which isn't published on crates.io. Runs under the `wstd` reactor
/// the engine starts for every WASI component.
async fn fetch_json(url: &str) -> Result<serde_json::Value> {
    let request = Request::get(url).body(Body::empty())?;
    let mut response = Client::new().send(request).await?;
    let bytes = response.body_mut().contents().await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Pluck `<coin>.usd` (f64) and `<coin>.last_updated_at` (u64) out of the
/// response and convert the price to a 7-decimal `i128` (`(p * 1e7).round()`).
fn extract(
    body: &serde_json::Value,
    coin: &str,
    label: &'static str,
) -> Result<(&'static str, u64, i128)> {
    let entry = body
        .get(coin)
        .ok_or_else(|| anyhow!("coingecko response missing `{coin}`"))?;

    let price = entry
        .get("usd")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| anyhow!("`{coin}.usd` not an f64"))?;
    if !price.is_finite() || price <= 0.0 {
        return Err(anyhow!(
            "`{coin}.usd` = {price}, expected a positive finite number"
        ));
    }

    let ts = entry
        .get("last_updated_at")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("`{coin}.last_updated_at` not a u64"))?;

    let price_e7 = (price * 1e7).round() as i128;
    Ok((label, ts, price_e7))
}
