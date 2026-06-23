//! Decode a Sepolia `TwapRequested(string,uint32,address)` event log into
//! the typed bundle the rest of the circuit needs.
//!
//! Event shape (`TwapTrigger.sol`):
//!   topic[0] = keccak256("TwapRequested(string,uint32,address)")
//!            = 0xa02a28cf01f2361e6c38ce664ac5287b7b3dbb7d6368d3646d5e865d82cfa88a
//!   topic[1] = abi.encode(indexed address requester) — 32-byte left-padded.
//!   data     = abi.encode(string asset, uint32 rangeSecs):
//!              word 0 = 0x40                 (offset to the dynamic string)
//!              word 1 = rangeSecs as u256    (low 4 bytes hold the value)
//!              word 2 = string length L      (u256)
//!              bytes  = the UTF-8 asset string, right-padded to a 32-byte boundary.
//!
//! Decoding is by hand — alloy/ethers would dwarf the rest of the
//! circuit. The shape is fixed by the Solidity event signature, so a
//! handful of `from_be_bytes` calls and length checks are enough.
//!
//! All errors are descriptive: the aggregator surfaces them verbatim, and
//! a malformed Sepolia log is much easier to debug from "rangeSecs has
//! non-zero high bytes" than from a generic "decode failed".

use anyhow::{anyhow, bail, Context, Result};

use crate::warpdrive::types::chain::EvmEventLog;

/// keccak256("TwapRequested(string,uint32,address)") — verified via
/// `cast keccak`; mirrored in `out/eth-trigger.json` after deploy. Any
/// other topic[0] is a different event and the circuit MUST bail rather
/// than mis-decode unrelated logs that the trigger filter accidentally
/// catches.
const TWAP_REQUESTED_TOPIC0: [u8; 32] = [
    0xa0, 0x2a, 0x28, 0xcf, 0x01, 0xf2, 0x36, 0x1e, 0x6c, 0x38, 0xce, 0x66, 0x4a, 0xc5, 0x28, 0x7b,
    0x7b, 0x3d, 0xbb, 0x7d, 0x63, 0x68, 0xd3, 0x64, 0x6d, 0x5e, 0x86, 0x5d, 0x82, 0xcf, 0xa8, 0x8a,
];

/// Decoded `TwapRequested` log, ready for `payload::encode_bridge`. Every
/// field is a deterministic function of the on-chain log so the same
/// `DecodedEvent` falls out for every operator that sees the same tx.
pub struct DecodedEvent {
    pub asset: String,
    pub range_secs: u32,
    /// `msg.sender` extracted from the 32-byte left-padded indexed
    /// address in topic[1]; the eventual `BridgeTriggerPayload::eth_origin`.
    pub eth_origin: [u8; 20],
    /// The Sepolia transaction hash, reused as both the on-chain
    /// `eth_tx_hash` audit field and the `event_id_salt` so the
    /// aggregator's QuorumQueue collapses both operators onto one envelope.
    pub eth_tx_hash: [u8; 32],
    /// Sepolia `block.timestamp` at emission. REQUIRED by the on-chain
    /// payload; this is the deterministic "requested_at" used downstream.
    pub block_timestamp: u64,
    /// Sepolia block number — used as `WasmResponse.ordering` so the
    /// host orders concurrent triggers the same way every operator does.
    pub block_number: u64,
}

pub fn parse_twap_requested(log: &EvmEventLog) -> Result<DecodedEvent> {
    let topics = &log.data.topics;
    if topics.len() != 2 {
        bail!(
            "expected 2 topics (topic0 + indexed requester), got {}",
            topics.len()
        );
    }

    // topic[0] — event signature hash. The trigger filter is configured
    // with this hash, but defence in depth: a misconfigured workflow
    // could hand us a different event and we'd silently misinterpret it.
    if topics[0].as_slice() != TWAP_REQUESTED_TOPIC0 {
        bail!(
            "topic[0] != TwapRequested signature hash (got 0x{})",
            hex_lower(&topics[0])
        );
    }

    // topic[1] — `address indexed requester`. ABI rule: addresses get
    // left-padded with 12 zero bytes into the 32-byte word.
    if topics[1].len() != 32 {
        bail!(
            "topic[1] expected 32 bytes (left-padded address), got {}",
            topics[1].len()
        );
    }
    let mut eth_origin = [0u8; 20];
    eth_origin.copy_from_slice(&topics[1][12..32]);

    // The non-indexed payload — (string asset, uint32 rangeSecs).
    let data = &log.data.data;
    // Need at least the 3 header words (offset, rangeSecs, string-length).
    if data.len() < 96 {
        bail!(
            "event data too short for (string, uint32) header: {} bytes",
            data.len()
        );
    }

    // Word 0 — offset to the dynamic `string`. For a `(string, uint32)`
    // tuple it is always 0x40 (the two-word head). Reject anything else
    // rather than seek to an attacker-controlled offset.
    let offset = read_u256_as_usize(&data[0..32]).context("decode string offset word")?;
    if offset != 64 {
        bail!("expected dynamic-string offset 0x40 (64), got {offset}");
    }

    // Word 1 — `uint32 rangeSecs` right-padded into 32 bytes. Upper 28
    // bytes MUST be zero or the value exceeds u32 and is a different
    // event from the one we know how to interpret.
    if data[32..60].iter().any(|&b| b != 0) {
        bail!("rangeSecs has non-zero high bytes (overflow u32)");
    }
    let mut range_buf = [0u8; 4];
    range_buf.copy_from_slice(&data[60..64]);
    let range_secs = u32::from_be_bytes(range_buf);

    // At `offset` (= 64): u256 string length, then the UTF-8 bytes
    // right-padded to a 32-byte boundary.
    let str_len =
        read_u256_as_usize(&data[64..96]).context("decode string length word")?;
    if str_len > u32::MAX as usize {
        bail!("asset string length exceeds u32: {str_len}");
    }
    let body_start = 96usize;
    let body_end = body_start
        .checked_add(str_len)
        .ok_or_else(|| anyhow!("asset string length overflow: {str_len}"))?;
    if data.len() < body_end {
        bail!(
            "event data truncated: need {body_end} bytes for string body, have {}",
            data.len()
        );
    }
    let asset = std::str::from_utf8(&data[body_start..body_end])
        .context("asset string not valid UTF-8")?
        .to_string();

    // tx_hash is `list<u8>` in WIT — always 32 bytes for an EVM tx, but
    // the host doesn't constrain that statically, so check.
    if log.tx_hash.len() != 32 {
        bail!("tx_hash expected 32 bytes, got {}", log.tx_hash.len());
    }
    let mut eth_tx_hash = [0u8; 32];
    eth_tx_hash.copy_from_slice(&log.tx_hash);

    // Sepolia public RPC always populates block_timestamp; treating None
    // as a hard error keeps the bridge deterministic — silently
    // substituting wall-clock time would diverge operators.
    let block_timestamp = log
        .block_timestamp
        .ok_or_else(|| anyhow!("Sepolia log missing block_timestamp"))?;

    Ok(DecodedEvent {
        asset,
        range_secs,
        eth_origin,
        eth_tx_hash,
        block_timestamp,
        block_number: log.block_number,
    })
}

/// Decode a 32-byte big-endian word as `usize`. Bails if the high 24
/// bytes are non-zero (the value would overflow a 64-bit host, never
/// mind a wasm32 one).
fn read_u256_as_usize(word: &[u8]) -> Result<usize> {
    debug_assert_eq!(word.len(), 32);
    if word[0..24].iter().any(|&b| b != 0) {
        bail!("u256 value too large for usize (high bytes non-zero)");
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&word[24..32]);
    Ok(u64::from_be_bytes(buf) as usize)
}

/// Lowercase-hex render for diagnostics — no hex crate, just a few bytes.
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
