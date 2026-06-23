import { CSSProperties, useEffect, useState } from "react";

import {
  Attestation,
  formatTwap,
  readQuorum,
  readRound2Bundle,
  shortHex,
} from "../lib/oracle";

const POLL_INTERVAL_MS = 2000;

interface Round2BundleProps {
  oracleId: string | null;
  requestId: bigint | null;
}

interface QuorumInfo {
  total: number;
}

export function Round2Bundle({ oracleId, requestId }: Round2BundleProps) {
  const [attestations, setAttestations] = useState<Attestation[]>([]);
  const [quorum, setQuorum] = useState<QuorumInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Refresh the quorum denominator once per `(oracleId)` — it's the
  // total registered signer count, which doesn't change between calls.
  useEffect(() => {
    if (!oracleId) return;
    let cancelled = false;
    void readQuorum(oracleId)
      .then((q) => {
        if (!cancelled) setQuorum({ total: q.denominator });
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

  const total = quorum?.total ?? Math.max(attestations.length, 5);
  const filled = Math.min(attestations.length, total);
  const pct = total > 0 ? Math.round((filled / total) * 100) : 0;
  const waitingSlots = Math.max(total - attestations.length, 0);

  return (
    <section className="section">
      <div className="bundle-head">
        <h2 className="section-title" style={{ margin: 0 }}>
          Round 2 attestations · request {requestId.toString()}
        </h2>
        <div className="bundle-progress">
          <span>
            {filled}/{quorum ? total : "?"} attestations
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
