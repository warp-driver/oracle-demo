import { OracleEvent, OracleEventKind } from "../lib/oracle";

interface EventStreamProps {
  events: OracleEvent[];
}

const KIND_DOT: Record<OracleEventKind, string> = {
  "twap-request": "🟡",
  "round2-ready": "🟢",
  finalized: "🟢",
};

const KIND_LABEL: Record<OracleEventKind, string> = {
  "twap-request": "TwapRequest",
  "round2-ready": "Round2Ready",
  finalized: "Finalized",
};

export function EventStream({ events }: EventStreamProps) {
  // Show newest 10 first. `events` is whatever App provides; the App
  // itself is responsible for accumulating across polls.
  const recent = events.slice(-10).reverse();

  return (
    <section className="section">
      <h2 className="section-title">Recent events</h2>
      {recent.length === 0 ? (
        <div className="events-empty">
          No OracleContract events yet — connect Freighter and submit a request.
        </div>
      ) : (
        <div className="event-list">
          {recent.map((ev) => (
            <div className="event-row" key={ev.id}>
              <span aria-hidden>{KIND_DOT[ev.kind]}</span>
              <span className="event-time">
                {ev.ledgerClosedAt.toLocaleTimeString()}
              </span>
              <span className="event-kind">{KIND_LABEL[ev.kind]}</span>
              <span className="event-detail">
                request {ev.requestId.toString()} · {summarise(ev)}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function summarise(ev: OracleEvent): string {
  if (ev.kind === "twap-request") {
    const asset = typeof ev.data.asset === "string" ? ev.data.asset : "?";
    const range =
      typeof ev.data.range_secs === "number" ||
      typeof ev.data.range_secs === "bigint"
        ? Number(ev.data.range_secs)
        : null;
    return range !== null ? `${asset}, ${range}s` : asset;
  }
  if (ev.kind === "round2-ready") {
    const asset = typeof ev.data.asset === "string" ? ev.data.asset : "?";
    return `${asset} bundle released`;
  }
  // finalized
  const med = ev.data.median;
  if (typeof med === "bigint") {
    return `median ${med.toString()}`;
  }
  return "finalized";
}
