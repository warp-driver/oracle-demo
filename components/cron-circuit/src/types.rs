use serde::{Deserialize, Serialize};

/// One price observation in a per-asset rolling window.
///
/// Persisted with `bincode` 1.3 — a positional, fixed-layout encoding
/// (8-byte little-endian `u64` + 16-byte little-endian `i128`). The
/// twap-circuit re-declares this struct with the identical field order
/// and `serde` derives so it can decode the bytes verbatim.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Sample {
    pub ts: u64,
    pub price_e7: i128,
}
