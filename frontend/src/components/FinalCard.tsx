import { useEffect, useState } from "react";

import {
  OracleEvent,
  findFinalizedMeta,
  formatTwap,
  readFinalTwap,
} from "../lib/oracle";

const POLL_INTERVAL_MS = 2000;

interface FinalCardProps {
  oracleId: string | null;
  requestId: bigint | null;
  events: OracleEvent[];
}

export function FinalCard({ oracleId, requestId, events }: FinalCardProps) {
  const [median, setMedian] = useState<bigint | null>(null);

  useEffect(() => {
    if (!oracleId || requestId === null) {
      setMedian(null);
      return;
    }
    let cancelled = false;
    let timer: number | undefined;
    async function tick() {
      if (cancelled || !oracleId || requestId === null) return;
      try {
        const v = await readFinalTwap(oracleId, requestId);
        if (cancelled) return;
        setMedian(v);
        if (v !== null) return; // stop polling once we have the answer
      } catch {
        // swallow — keep retrying; bubbled errors get noisy fast.
      }
      if (!cancelled) timer = window.setTimeout(tick, POLL_INTERVAL_MS);
    }
    void tick();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [oracleId, requestId]);

  if (requestId === null) return null;
  if (median === null) {
    return (
      <section className="section">
        <h2 className="section-title">Final result</h2>
        <div className="bundle-empty">
          Waiting for the median circuit to publish a quorum-signed final TWAP.
        </div>
      </section>
    );
  }

  const meta = findFinalizedMeta(events, requestId);
  const ts =
    meta.computedAt !== null
      ? new Date(Number(meta.computedAt) * 1000).toISOString()
      : null;

  return (
    <section className="section">
      <h2 className="section-title">Final result</h2>
      <div className="final-card">
        <span className="final-label">Median TWAP</span>
        <span className="final-value">{formatTwap(median)} USD</span>
        <div className="final-meta">
          <span>
            attestations:{" "}
            <strong>{meta.nAttestations ?? "—"}</strong>
          </span>
          <span>
            computed_at: <strong>{ts ?? "—"}</strong>
          </span>
        </div>
      </div>
    </section>
  );
}
