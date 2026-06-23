//! eth-bridge-circuit — observes Sepolia `TwapRequested` events from the
//! `TwapTrigger` contract and emits a Stellar-bound
//! `SubmissionPayload::BridgeTrigger(BridgeTriggerPayload)` the aggregator
//! wraps into an `XlmEnvelope` and submits via `OracleContract::verify_xlm`.
//!
//! Full-quorum bridge: both operators independently observe the same
//! Sepolia transaction, decode it into byte-identical XDR payloads (the
//! decode is a pure function of the on-chain log), and share the same
//! 32-byte tx hash as `event_id_salt`. The aggregator's QuorumQueue then
//! collapses both signatures onto the same envelope, mirroring how the
//! median circuit's salt collapses its operators onto one Final
//! attestation.
//!
//! No HTTP, no KV, no signing — the circuit is a thin event decoder.

mod payload;
mod trigger;

wit_bindgen::generate!({
    world: "circuit-world",
    path: "../../wit-definitions/wit",
    generate_all,
});

use warpdrive::vectr::input::TriggerData;

struct Component;

impl Guest for Component {
    fn run(t: TriggerAction) -> Result<Vec<WasmResponse>, String> {
        run_inner(t).map_err(|e| format!("eth-bridge-circuit: {e:#}"))
    }
}

fn run_inner(trigger_action: TriggerAction) -> anyhow::Result<Vec<WasmResponse>> {
    let evm = match trigger_action.data {
        TriggerData::EvmContractEvent(e) => e,
        _ => anyhow::bail!("expected EvmContractEvent trigger"),
    };

    let decoded = trigger::parse_twap_requested(&evm.log)?;
    let payload_bytes = payload::encode_bridge(&decoded)?;

    // `ordering` = Sepolia block number so the host pipeline can
    // serialise concurrent observations the same way every operator
    // would; `event_id_salt` = the 32-byte tx hash so both operators
    // land in the same QuorumQueue bucket and their signatures
    // collapse onto a single envelope.
    Ok(vec![WasmResponse {
        payload: payload_bytes,
        ordering: Some(decoded.block_number),
        event_id_salt: Some(decoded.eth_tx_hash.to_vec()),
    }])
}

export!(Component);
