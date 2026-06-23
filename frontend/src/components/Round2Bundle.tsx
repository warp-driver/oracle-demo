import { CSSProperties, useEffect, useState } from "react";

import {
  Attestation,
  formatTwap,
  readQuorum,
  readRound2Bundle,
  readSignerCount,
  shortHex,
} from "../lib/oracle";

const POLL_INTERVAL_MS = 2000;

interface Round2BundleProps {
  oracleId: string | null;
  requestId: bigint | null;
}

interface QuorumInfo {
  /** Total ed25519 signers registered on the security contract. */
  signerCount: number;
  /** The round-2 release threshold the OracleContract applies:
   *  `ceil(signerCount * numerator / denominator)`. */
  threshold: number;
}

export function Round2Bundle({ oracleId, requestId }: Round2BundleProps) {
  const [attestations, setAttestations] = useState<Attestation[]>([]);
  const [quorum, setQuorum] = useState<QuorumInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Pull the live signer count and the contract's quorum fraction, then
  // derive the actual round-2 threshold the OracleContract applies
  // (`ceil(signers * num / denom)`). With one operator at 4/5 the
  // threshold is 1, NOT 5 — the denominator is a ratio, not a slot
  // count.
  useEffect(() => {
    if (!oracleId) return;
    let cancelled = false;
    void Promise.all([readQuorum(oracleId), readSignerCount(oracleId)])
      .then(([q, signers]) => {
        if (cancelled) return;
        const threshold = Math.max(
          1,
          Math.ceil((signers * q.numerator) / q.denominator),
        );
        setQuorum({ signerCount: signers, threshold });
      })
      .catch(() => {
        // Falls back to "n/?" in the ring — non-fatal.
      });
    return () => {
      cancelled = true;
    };
  }, [oracleId]);

  // Poll the bundle every 2s while we have an active request.
  useEffect(() => {
    if (!oracleId || requestId === null) {
      setAttestations([]);
      setError(null);
      return;
    }
    let cancelled = false;
    let timer: number | undefined;
    async function tick() {
      if (cancelled || !oracleId || requestId === null) return;
      try {
        const list = await readRound2Bundle(oracleId, requestId);
        if (cancelled) return;
        setAttestations(list);
        setError(null);
      } catch (err) {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        if (!cancelled) {
          timer = window.setTimeout(tick, POLL_INTERVAL_MS);
        }
      }
    }
    void tick();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [oracleId, requestId]);

  if (requestId === null) {
    return (
      <section className="section">
        <h2 className="section-title">Round 2 attestations</h2>
        <div className="bundle-empty">
          No active request — submit one above to watch attestations arrive.
        </div>
      </section>
    );
  }

  const slots = quorum?.signerCount ?? Math.max(attestations.length, 1);
  const threshold = quorum?.threshold ?? slots;
  const filled = Math.min(attestations.length, slots);
  const pct = threshold > 0 ? Math.round((filled / threshold) * 100) : 0;
  const waitingSlots = Math.max(slots - attestations.length, 0);

  return (
    <section className="section">
      <div className="bundle-head">
        <h2 className="section-title" style={{ margin: 0 }}>
          Round 2 attestations · request {requestId.toString()}
        </h2>
        <div className="bundle-progress">
          <span>
            {filled}/{threshold} signed
            {quorum && quorum.signerCount > quorum.threshold
              ? ` (of ${quorum.signerCount})`
              : ""}
          </span>
          <span
            className="ring"
            style={{ "--ring-pct": String(pct) } as CSSProperties}
            aria-hidden
          />
        </div>
      </div>

      <div className="bundle-list">
        {attestations.map((a) => (
          <div className="signer-row" key={shortHex(a.signer)}>
            <span className="signer-key">{shortHex(a.signer)} →</span>
            <span className="signer-value">
              {formatTwap(a.twap)} USD <span className="check">✓</span>
            </span>
          </div>
        ))}
        {Array.from({ length: waitingSlots }, (_, i) => (
          <div className="signer-row waiting" key={`pending-${i}`}>
            <span className="signer-key">(waiting)</span>
            <span className="signer-value">—</span>
          </div>
        ))}
        {attestations.length === 0 && waitingSlots === 0 && (
          <div className="bundle-empty">No attestations yet.</div>
        )}
      </div>
      {error && <p className="form-msg error">{error}</p>}
    </section>
  );
}
