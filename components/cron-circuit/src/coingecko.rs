use anyhow::{anyhow, Context, Result};
use wstd::http::{Body, Client, Request};
use wstd::runtime::block_on;

use crate::host;

/// CoinGecko simple-price endpoint. Demo keys (free, `CG-…` prefix)
/// AND no-key callers both hit `api.coingecko.com`; the Demo key just
/// rides along in the `x-cg-demo-api-key` header to lift the rate
/// limit from 10–30 req/min/IP to ~30k req/month. Only true Pro keys
/// (paid, no `CG-` prefix) target the dedicated `pro-api.coingecko.com`
/// host with `x-cg-pro-api-key` — not what this demo uses.
///
/// The host's HTTP allowlist (set in service.json as
/// `allowed_http_hosts = ["api.coingecko.com"]`) MUST authorise this
/// host or the request is rejected before it leaves the sandbox.
const URL: &str = "https://api.coingecko.com/api/v3/simple/price\
?ids=bitcoin,ethereum&vs_currencies=usd&include_last_updated_at=true";

/// Fetch the latest BTC/USD and ETH/USD spot prices.
///
/// Returns one `(label, ts, price_e7)` tuple per asset — BTC first,
/// then ETH. Errors include the underlying HTTP status + first 200
/// bytes of the response body so a rate-limit (429 with no `bitcoin`
/// key) is recognisable from the node log without external tooling.
pub fn fetch_now() -> Result<Vec<(&'static str, u64, i128)>> {
    let api_key = host::config_var("coingecko_api_key");
    let body = block_on(async { fetch(URL, api_key.as_deref()).await })
        .context("coingecko GET failed")?;
    let json: serde_json::Value = serde_json::from_slice(&body)
        .with_context(|| format!("coingecko response not JSON: {}", preview(&body)))?;
    Ok(vec![
        extract(&json, "bitcoin", "btc_usd", &body)?,
        extract(&json, "ethereum", "eth_usd", &body)?,
    ])
}

/// GET `url`, optionally with the CoinGecko Demo/Pro key header set,
/// return the raw body. Surfaces non-2xx status codes with the body
/// embedded so the caller can decide whether to retry / log / bail.
async fn fetch(url: &str, api_key: Option<&str>) -> Result<Vec<u8>> {
    let mut builder = Request::get(url)
        // CoinGecko's edge rejects requests with no UA (HTTP 403 with
        // the message 'Please add a descriptive User-Agent…').
        .header("user-agent", "warpdrive-oracle-demo/0.1 (+https://wa.dev/warpdrive)")
        .header("accept", "application/json");
    if let Some(key) = api_key {
        builder = builder.header("x-cg-demo-api-key", key);
    }
    let request = builder.body(Body::empty())?;
    let mut response = Client::new().send(request).await?;
    let status = response.status();
    let bytes = response.body_mut().contents().await?.to_vec();
    if !status.is_success() {
        return Err(anyhow!(
            "coingecko returned HTTP {} — body: {}",
            status.as_u16(),
            preview(&bytes)
        ));
    }
    Ok(bytes)
}

/// Pluck `<coin>.usd` (f64) and `<coin>.last_updated_at` (u64) out of
/// the parsed response and convert the price to a 7-decimal `i128`
/// (`(p * 1e7).round()`). On a missing-key failure we include the
/// raw body preview so a 200-OK rate-limit page (yes, CoinGecko does
/// that sometimes) is debuggable from the node log.
fn extract(
    body: &serde_json::Value,
    coin: &str,
    label: &'static str,
    raw: &[u8],
) -> Result<(&'static str, u64, i128)> {
    let entry = body.get(coin).ok_or_else(|| {
        anyhow!(
            "coingecko response missing `{coin}` — likely rate-limited; body: {}",
            preview(raw)
        )
    })?;

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

/// Trim raw bytes to a short, UTF-8-safe debug preview for inclusion
/// in error chains. Long binary responses don't blow up the log line.
fn preview(bytes: &[u8]) -> String {
    let limit = 200;
    let slice = if bytes.len() > limit { &bytes[..limit] } else { bytes };
    let text = String::from_utf8_lossy(slice);
    if bytes.len() > limit {
        format!("{text}… ({} bytes total)", bytes.len())
    } else {
        text.into_owned()
    }
}
