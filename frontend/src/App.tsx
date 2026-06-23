import { useEffect, useState } from "react";

import { EventStream } from "./components/EventStream";
import { FinalCard } from "./components/FinalCard";
import { Header } from "./components/Header";
import { RequestForm } from "./components/RequestForm";
import { Round2Bundle } from "./components/Round2Bundle";
import { currentAddress as currentFreighterAddress } from "./lib/freighter";
import { OracleEvent, tailEvents } from "./lib/oracle";

const EVENT_POLL_MS = 3000;
const EVENT_KEEP = 50;
const ORACLE_JSON_URL = "/oracle.json";

interface ResolvedOracleId {
  contractId: string;
  source: "public" | "env";
}

function isOracleJson(v: unknown): v is { oracle: string } {
  if (typeof v !== "object" || v === null) return false;
  if (!("oracle" in v)) return false;
  return typeof v.oracle === "string";
}

/**
 * Load the oracle contract address. Preferred source is
 * `/oracle.json` (copied by `task deploy-oracle` from `out/oracle.json`).
 * Falls back to `VITE_ORACLE_CONTRACT_ID` so dev setups without the
 * deploy script still work.
 */
async function loadOracleId(): Promise<ResolvedOracleId | null> {
  try {
    const resp = await fetch(ORACLE_JSON_URL, { cache: "no-cache" });
    if (resp.ok) {
      const ctype = resp.headers.get("content-type") ?? "";
      if (ctype.includes("application/json")) {
        const json: unknown = await resp.json();
        if (isOracleJson(json) && json.oracle.startsWith("C")) {
          return { contractId: json.oracle, source: "public" };
        }
      }
    }
  } catch {
    // fall through to env var fallback
  }
  const envId: unknown = import.meta.env.VITE_ORACLE_CONTRACT_ID;
  if (typeof envId === "string" && envId.startsWith("C")) {
    return { contractId: envId, source: "env" };
  }
  return null;
}

export function App() {
  const [oracleId, setOracleId] = useState<ResolvedOracleId | null>(null);
  const [oracleResolved, setOracleResolved] = useState(false);
  const [walletAddress, setWalletAddress] = useState<string | null>(null);
  const [requestId, setRequestId] = useState<bigint | null>(null);
  const [events, setEvents] = useState<OracleEvent[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Resolve the contract address and the already-authorised wallet
  // address on mount. Both are independent so we kick them off in
  // parallel.
  useEffect(() => {
    let cancelled = false;
    void loadOracleId().then((id) => {
      if (cancelled) return;
      setOracleId(id);
      setOracleResolved(true);
    });
    void currentFreighterAddress().then((addr) => {
      if (!cancelled && addr) setWalletAddress(addr);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Tail the OracleContract event log. We keep a rolling cursor so
  // each poll only fetches new events; events accumulate up to
  // EVENT_KEEP newest.
  useEffect(() => {
    if (!oracleId) return;
    let cancelled = false;
    let cursor: string | undefined;
    let timer: number | undefined;
    async function tick() {
      if (cancelled || !oracleId) return;
      try {
        const { events: fresh, cursor: nextCursor } = await tailEvents({
          oracleId: oracleId.contractId,
          cursor,
          limit: 30,
        });
        cursor = nextCursor;
        if (cancelled) return;
        if (fresh.length > 0) {
          setEvents((prev) => {
            const seen = new Set(prev.map((e) => e.id));
            const merged = [...prev];
            for (const ev of fresh) {
              if (!seen.has(ev.id)) merged.push(ev);
            }
            return merged.slice(-EVENT_KEEP);
          });
        }
      } catch (err) {
        // Don't tear the UI down for a transient RPC blip — surface
        // once, then keep retrying.
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      } finally {
        if (!cancelled) {
          timer = window.setTimeout(tick, EVENT_POLL_MS);
        }
      }
    }
    void tick();
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [oracleId]);

  return (
    <div className="app">
      <Header
        walletAddress={walletAddress}
        onWalletConnected={setWalletAddress}
        onError={setError}
      />

      {oracleResolved && !oracleId && (
        <div className="banner">
          Oracle contract not configured. Drop the deploy output into{" "}
          <code>frontend/public/oracle.json</code> or set{" "}
          <code>VITE_ORACLE_CONTRACT_ID</code> to enable on-chain reads.
        </div>
      )}
      {error && (
        <div className="banner error" onClick={() => setError(null)}>
          {error} <span style={{ float: "right" }}>×</span>
        </div>
      )}

      <RequestForm
        oracleId={oracleId?.contractId ?? null}
        walletAddress={walletAddress}
        onRequest={(id) => setRequestId(id)}
        onError={setError}
      />

      <Round2Bundle
        oracleId={oracleId?.contractId ?? null}
        requestId={requestId}
      />

      <FinalCard
        oracleId={oracleId?.contractId ?? null}
        requestId={requestId}
        events={events}
      />

      <EventStream events={events} />
    </div>
  );
}
