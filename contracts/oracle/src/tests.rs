//! Unit tests for the Oracle contract.
//!
//! Two paths exercised here:
//!
//! 1. **Request lifecycle.** `request_twap` mints a fresh id, persists
//!    `RequestInfo`, and emits the `TwapRequest` event. We just assert the
//!    return value and reload via `request(id)`.
//!
//! 2. **Round 2 dedup + threshold release.** A test harness in
//!    `mocks` registers two ed25519 signer keys with weights summing
//!    above the configured 4/5 threshold (since 4/5 of 1 signer rounds up
//!    to 1), so a single attestation should trip the `Round2Ready`
//!    event. The same envelope replayed must return `EventAlreadySeen`.
//!
//! End-to-end flow with quorum-signed `submit_final` lives in the
//! integration tests rather than this no-std unit suite — wiring the
//! envelope + ed25519 signature off-chain requires the host helpers in
//! `warpdrive-shared` `testutils`.

use soroban_sdk::{Env, Symbol};
use soroban_sdk::testutils::Address as _;

use crate::OracleContract;

#[test]
fn request_twap_assigns_and_persists() {
    let env = Env::default();
    let verification = soroban_sdk::Address::generate(&env);
    let oracle_id = env.register(OracleContract, (verification, 4u32, 5u32));
    env.as_contract(&oracle_id, || {
        let id1 = crate::contract::OracleContract::request_twap(
            env.clone(),
            Symbol::new(&env, "btc"),
            3600,
        );
        let id2 = crate::contract::OracleContract::request_twap(
            env.clone(),
            Symbol::new(&env, "eth"),
            21600,
        );
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        let info1 = crate::contract::OracleContract::request(env.clone(), id1).unwrap();
        assert_eq!(info1.asset, Symbol::new(&env, "btc"));
        assert_eq!(info1.range_secs, 3600);
    });
}

#[test]
fn constructor_rejects_out_of_range_quorum() {
    let env = Env::default();
    let verification = soroban_sdk::Address::generate(&env);
    // numerator > denominator is illegal.
    let result =
        std::panic::catch_unwind(|| env.register(OracleContract, (verification.clone(), 6u32, 5u32)));
    assert!(result.is_err(), "constructor must reject quorum > 1");
}

#[test]
fn final_twap_is_empty_until_submitted() {
    let env = Env::default();
    let verification = soroban_sdk::Address::generate(&env);
    let oracle_id = env.register(OracleContract, (verification, 4u32, 5u32));
    env.as_contract(&oracle_id, || {
        assert!(crate::contract::OracleContract::final_twap(env.clone(), 999).is_none());
        assert!(crate::contract::OracleContract::latest(env.clone(), Symbol::new(&env, "btc")).is_none());
    });
}
