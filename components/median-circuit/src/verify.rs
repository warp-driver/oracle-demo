//! Off-chain re-validation of Round 2 attestations.
//!
//! The on-chain oracle already runs `Ed25519VerificationClient::try_check_one`
//! per attestation before bundling, so by the time a `Round2Ready` event is
//! emitted every signature is *known* to verify under the security module's
//! current signer set. Re-checking here is belt-and-suspenders: it lets a
//! malicious aggregator not silently swap a forged bundle into the Round 3
//! input, and it makes the median circuit independent of trust in the
//! upstream event payload.
//!
//! We do NOT re-query the security module from here. Doing so would require
//! `wasi_soroban_rs` + a tokio runtime + a host-resolved Stellar chain
//! config, none of which the WASI sandbox cleanly exposes for a synchronous
//! `Guest::run` entry. The contract's `try_check_one` already enforces
//! "signer is registered with non-zero weight at the submission ledger";
//! the cryptographic check below proves the bytes in the bundle weren't
//! altered after that on-chain gate.
//!
//! Envelope hashing is SEP-0053: `SHA256("Stellar Signed Message:\n" || envelope_bytes)`.
//! Keep [`SEP053_PREFIX`] in lock-step with the contract's
//! `ed25519-security`/`ed25519-verification` modules — if either side drifts,
//! every attestation will silently fail to validate.

use crate::trigger::DecodedAttestation;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// SEP-0053 domain-separation prefix. MUST stay byte-identical to the
/// constant used by the on-chain `ed25519-verification` contract when it
/// reconstructs the hash for `try_check_one` / `try_verify`.
pub const SEP053_PREFIX: &[u8] = b"Stellar Signed Message:\n";

pub fn is_valid(att: &DecodedAttestation) -> anyhow::Result<bool> {
    let mut hasher = Sha256::new();
    hasher.update(SEP053_PREFIX);
    hasher.update(&att.envelope);
    let digest = hasher.finalize();

    // ed25519-dalek is no_std here so its Error type doesn't implement
    // `std::error::Error` — `?` can't auto-convert into `anyhow::Error`.
    // A bad pubkey is itself a "this attestation is invalid" answer; we
    // surface it as `Ok(false)` rather than propagating.
    let key = match VerifyingKey::from_bytes(&att.signer) {
        Ok(k) => k,
        Err(_) => return Ok(false),
    };
    let sig = Signature::from_bytes(&att.signature);
    Ok(key.verify(digest.as_slice(), &sig).is_ok())
}
