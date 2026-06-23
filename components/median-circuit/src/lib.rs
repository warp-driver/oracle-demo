//! Round 3 (median) circuit.
//!
//! Triggered by `OracleContract::Round2Ready`, this circuit:
//!   1. Decodes the bundle of single-signer Round 2 attestations.
//!   2. Re-verifies each attestation's ed25519 signature off-chain
//!      ([`verify::is_valid`]). The contract already enforced
//!      "signer registered with non-zero weight" via `try_check_one`
//!      when accepting each attestation, so this purely defends against
//!      a malicious aggregator handing the circuit a forged bundle
//!      between Round 2 and Round 3.
//!   3. Takes the median of the surviving TWAPs (even-count → average
//!      of the two middles, odd-count → the exact middle).
//!   4. XDR-encodes a `FinalPayload` and returns it as the sole
//!      `WasmResponse`. Salt = `request_id LE bytes ++ b"-median"` so
//!      every Vectr produces the same `event_id`, letting the host
//!      collect a quorum signature batch on the same envelope.

mod payload;
mod trigger;
mod verify;

wit_bindgen::generate!({
    world: "circuit-world",
    path: "../../wit-definitions/wit",
    generate_all,
});

use warpdrive::vectr::input::TriggerData;

struct Component;

impl Guest for Component {
    fn run(t: TriggerAction) -> Result<Vec<WasmResponse>, String> {
        run_inner(t).map_err(|e| format!("median-circuit: {e:#}"))
    }
}

fn run_inner(trigger_action: TriggerAction) -> anyhow::Result<Vec<WasmResponse>> {
    let event = match trigger_action.data {
        TriggerData::StellarContractEvent(e) => e.event,
        _ => anyhow::bail!("expected StellarContractEvent trigger"),
    };

    let bundle = trigger::parse_round2_ready(&event)?;

    // Collect the valid TWAPs *and* their attestation timestamps. We
    // need the latter for a deterministic `computed_at` field — using
    // `wall_clock::now()` would differ by milliseconds across operators
    // and produce different envelope bytes for the same `event_id`,
    // which blocks signature quorum.
    let (valid_twaps, valid_times): (Vec<i128>, Vec<u64>) = bundle
        .attestations
        .iter()
        .filter(|a| verify::is_valid(a).unwrap_or(false))
        .map(|a| (a.twap, a.computed_at))
        .unzip();
    if valid_twaps.is_empty() {
        anyhow::bail!("no valid Round 2 attestations");
    }

    let median = compute_median(&valid_twaps);
    // Latest attestation timestamp — same set across operators, so the
    // resulting payload bytes are byte-identical and the on-chain queue
    // can collapse the per-operator signatures into one envelope.
    let computed_at = *valid_times.iter().max().expect("non-empty by check above");

    let payload_bytes = payload::encode_final(
        &bundle.asset,
        bundle.request_id,
        median,
        valid_twaps.len() as u32,
        computed_at,
    )?;

    let mut salt = bundle.request_id.to_le_bytes().to_vec();
    salt.extend_from_slice(b"-median");

    Ok(vec![WasmResponse {
        payload: payload_bytes,
        ordering: Some(bundle.request_id),
        event_id_salt: Some(salt),
    }])
}

fn compute_median(values: &[i128]) -> i128 {
    let mut v = values.to_vec();
    v.sort();
    let n = v.len();
    if n.is_multiple_of(2) {
        // Even count: average the two middle values. `(a + b) / 2` is
        // safe here because all attestation TWAPs are bounded i128 prices
        // (7-decimal-scaled USD) — addition can't overflow at any
        // realistic scale.
        (v[n / 2 - 1] + v[n / 2]) / 2
    } else {
        v[n / 2]
    }
}

export!(Component);
