// Soroban / Stellar RPC client for the `OracleContract` deployed by
// `task deploy-oracle`. All on-chain reads go through
// `simulateTransaction` against the contract methods; writes go through
// Freighter for signing then `sendTransaction` + `pollTransaction`.
//
// We deliberately avoid the auto-generated TypeScript bindings — the
// surface here is small enough (one write entry, four reads, one event
// query) that a hand-written client is clearer than the generated noise.

import {
  Account,
  Contract,
  Networks,
  TransactionBuilder,
  nativeToScVal,
  scValToNative,
  rpc,
  xdr,
} from "@stellar/stellar-sdk";

import { signTransaction } from "./freighter";

export const RPC_URL = "https://soroban-testnet.stellar.org";
export const NETWORK = Networks.TESTNET;
export const server = new rpc.Server(RPC_URL);

const TOPIC_TWAP_REQUEST = "twapreq";
const TOPIC_ROUND2_READY = "r2ready";
const TOPIC_FINALIZED = "finaltwp";

// ─── domain types ────────────────────────────────────────────────────

export type Asset = "btc-usd" | "eth-usd";

export interface Attestation {
  signer: Uint8Array;
  signature: Uint8Array;
  envelope: Uint8Array;
  twap: bigint;
  computedAt: bigint;
}

export interface FinalResult {
  median: bigint;
  /** Filled when we can match a `Finalized` event for this request. */
  nAttestations: number | null;
  computedAt: bigint | null;
}

export type OracleEventKind = "twap-request" | "round2-ready" | "finalized";

export interface OracleEvent {
  id: string;
  kind: OracleEventKind;
  requestId: bigint;
  /** Wall-clock when the ledger that emitted this closed. */
  ledgerClosedAt: Date;
  /** Decoded structured payload — exact shape varies by `kind`. */
  data: Record<string, unknown>;
}

// ─── tiny type-guards (kept here so callers don't repeat them) ───────

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function asBigInt(v: unknown): bigint | null {
  if (typeof v === "bigint") return v;
  if (typeof v === "number" && Number.isInteger(v)) return BigInt(v);
  return null;
}

function asNumber(v: unknown): number | null {
  if (typeof v === "number") return v;
  if (typeof v === "bigint") return Number(v);
  return null;
}

function asString(v: unknown): string | null {
  return typeof v === "string" ? v : null;
}

function asBytes(v: unknown): Uint8Array | null {
  // node Buffer also extends Uint8Array, so this single check suffices.
  return v instanceof Uint8Array ? v : null;
}

// ─── reads ───────────────────────────────────────────────────────────

// Synthetic source account for read simulations. The RPC does not
// validate the source account's existence or sequence during
// `simulateTransaction` — building a TransactionBuilder just needs a
// `string`-typed account id and sequence to compute the hash. We
// borrow the same well-known sentinel that blend-ui uses so failures
// are easy to recognise.
//
// Operators can override via `VITE_SIMULATION_SOURCE` if their custom
// RPC enforces stricter checks.
const ENV_SIMULATION_SOURCE: unknown = import.meta.env.VITE_SIMULATION_SOURCE;
export const SIMULATION_SOURCE =
  typeof ENV_SIMULATION_SOURCE === "string" && ENV_SIMULATION_SOURCE.length > 0
    ? ENV_SIMULATION_SOURCE
    : "GANXGJV2RNOFMOSQ2DTI3RKDBAVERXUVFC27KW3RLVQCLB3RYNO3AAI4";

/** Run a read-only contract call via simulation and return its ScVal. */
async function simulateRead(
  oracleId: string,
  method: string,
  args: xdr.ScVal[],
): Promise<xdr.ScVal> {
  const sourceAccount = new Account(SIMULATION_SOURCE, "0");
  const tx = new TransactionBuilder(sourceAccount, {
    fee: "100",
    networkPassphrase: NETWORK,
  })
    .addOperation(new Contract(oracleId).call(method, ...args))
    .setTimeout(30)
    .build();
  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) {
    throw new Error(`simulate ${method} failed: ${sim.error}`);
  }
  if (!sim.result) {
    throw new Error(`simulate ${method} returned no result`);
  }
  return sim.result.retval;
}

/** `oracle.final_twap(request_id) -> Option<i128>` */
export async function readFinalTwap(
  oracleId: string,
  requestId: bigint,
): Promise<bigint | null> {
  const scv = await simulateRead(oracleId, "final_twap", [
    nativeToScVal(requestId, { type: "u64" }),
  ]);
  const native: unknown = scValToNative(scv);
  if (native === null || native === undefined) return null;
  return asBigInt(native);
}

/** `oracle.round2_bundle(request_id) -> Round2Bundle` */
export async function readRound2Bundle(
  oracleId: string,
  requestId: bigint,
): Promise<Attestation[]> {
  const scv = await simulateRead(oracleId, "round2_bundle", [
    nativeToScVal(requestId, { type: "u64" }),
  ]);
  const native: unknown = scValToNative(scv);
  if (!isRecord(native)) return [];
  const list = native.attestations;
  if (!Array.isArray(list)) return [];
  const out: Attestation[] = [];
  for (const item of list) {
    if (!isRecord(item)) continue;
    const signer = asBytes(item.signer);
    const signature = asBytes(item.signature);
    const envelope = asBytes(item.envelope);
    const twap = asBigInt(item.twap);
    const computedAt = asBigInt(item.computed_at);
    if (!signer || !signature || !envelope || twap === null || computedAt === null) {
      continue;
    }
    out.push({ signer, signature, envelope, twap, computedAt });
  }
  return out;
}

/** `oracle.quorum() -> (u32, u32)` */
export async function readQuorum(
  oracleId: string,
): Promise<{ numerator: number; denominator: number }> {
  const scv = await simulateRead(oracleId, "quorum", []);
  const native: unknown = scValToNative(scv);
  if (!Array.isArray(native) || native.length !== 2) {
    throw new Error("quorum() returned unexpected shape");
  }
  const num = asNumber(native[0]);
  const den = asNumber(native[1]);
  if (num === null || den === null) {
    throw new Error("quorum() returned non-numeric values");
  }
  return { numerator: num, denominator: den };
}

// ─── write (request_twap) ────────────────────────────────────────────

export interface RequestTwapResult {
  requestId: bigint;
  txHash: string;
}

/**
 * Build, sign (via Freighter), submit, and poll a
 * `request_twap(asset, range_secs)` invocation. Returns the new
 * `request_id` emitted by the contract.
 */
export async function requestTwap(args: {
  oracleId: string;
  walletAddress: string;
  asset: Asset;
  rangeSecs: number;
}): Promise<RequestTwapResult> {
  const account = await server.getAccount(args.walletAddress);
  const tx = new TransactionBuilder(account, {
    fee: "1000",
    networkPassphrase: NETWORK,
  })
    .addOperation(
      new Contract(args.oracleId).call(
        "request_twap",
        nativeToScVal(args.asset, { type: "symbol" }),
        nativeToScVal(args.rangeSecs, { type: "u32" }),
      ),
    )
    .setTimeout(30)
    .build();

  // Surface simulation errors before we ask the user to sign.
  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) {
    throw new Error(`simulate request_twap failed: ${sim.error}`);
  }

  const prepared = await server.prepareTransaction(tx);
  const { signedTxXdr } = await signTransaction(prepared.toXDR(), {
    network: NETWORK,
    address: args.walletAddress,
  });

  const finalTx = TransactionBuilder.fromXDR(signedTxXdr, NETWORK);
  const send = await server.sendTransaction(finalTx);
  if (send.status === "ERROR") {
    throw new Error(`sendTransaction error: ${send.hash}`);
  }
  const settled = await server.pollTransaction(send.hash, { attempts: 30 });
  if (settled.status !== rpc.Api.GetTransactionStatus.SUCCESS) {
    throw new Error(`transaction ${send.hash} ended ${settled.status}`);
  }
  if (!settled.returnValue) {
    throw new Error(`transaction ${send.hash} had no return value`);
  }
  const native: unknown = scValToNative(settled.returnValue);
  const id = asBigInt(native);
  if (id === null) {
    throw new Error(`request_twap returned non-u64: ${String(native)}`);
  }
  return { requestId: id, txHash: send.hash };
}

// ─── events ──────────────────────────────────────────────────────────

function classifyTopic(symbol: string): OracleEventKind | null {
  if (symbol === TOPIC_TWAP_REQUEST) return "twap-request";
  if (symbol === TOPIC_ROUND2_READY) return "round2-ready";
  if (symbol === TOPIC_FINALIZED) return "finalized";
  return null;
}

/**
 * Tail the most recent OracleContract events. Caller passes the
 * `cursor` returned from a previous call to avoid re-fetching. On the
 * first call, omit `cursor` and we'll fetch from ~1000 ledgers back.
 */
export async function tailEvents(args: {
  oracleId: string;
  cursor?: string;
  limit?: number;
}): Promise<{ events: OracleEvent[]; cursor: string }> {
  const filters: rpc.Api.EventFilter[] = [
    { type: "contract", contractIds: [args.oracleId] },
  ];
  const request: rpc.Server.GetEventsRequest = {
    filters,
    limit: args.limit ?? 50,
  };
  if (args.cursor) {
    request.cursor = args.cursor;
  } else {
    const latest = await server.getLatestLedger();
    // Testnet closes a ledger ~every 5s, so 1000 ledgers ≈ 80 minutes
    // — plenty for the demo's recent-events ribbon.
    request.startLedger = Math.max(1, latest.sequence - 1000);
  }
  const resp = await server.getEvents(request);
  const out: OracleEvent[] = [];
  for (const ev of resp.events) {
    if (ev.topic.length < 2) continue;
    const topicSym: unknown = scValToNative(ev.topic[0]);
    const sym = asString(topicSym);
    if (!sym) continue;
    const kind = classifyTopic(sym);
    if (!kind) continue;
    const reqIdNative: unknown = scValToNative(ev.topic[1]);
    const reqId = asBigInt(reqIdNative);
    if (reqId === null) continue;
    const value: unknown = scValToNative(ev.value);
    out.push({
      id: ev.id,
      kind,
      requestId: reqId,
      ledgerClosedAt: new Date(ev.ledgerClosedAt),
      data: isRecord(value) ? value : { value },
    });
  }
  return { events: out, cursor: resp.cursor };
}

/**
 * Sweep `events` for a `finalized` event matching `requestId` and
 * surface its `n_attestations` + `computed_at`. The median is
 * authoritative from the read call; this just fleshes out the card.
 */
export function findFinalizedMeta(
  events: OracleEvent[],
  requestId: bigint,
): { nAttestations: number | null; computedAt: bigint | null } {
  for (const ev of events) {
    if (ev.kind !== "finalized" || ev.requestId !== requestId) continue;
    return {
      nAttestations: asNumber(ev.data.n_attestations),
      computedAt: asBigInt(ev.data.computed_at),
    };
  }
  return { nAttestations: null, computedAt: null };
}

// ─── formatters ──────────────────────────────────────────────────────

/** TWAPs are 7-decimal-scaled i128. */
export function formatTwap(scaled: bigint): string {
  const scale = 10_000_000n;
  const whole = scaled / scale;
  const frac = scaled % scale;
  const fracStr = frac.toString().padStart(7, "0").slice(0, 4);
  // Group thousands in the integer part for readability.
  const wholeStr = whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, "_");
  return `${wholeStr}.${fracStr}`;
}

export function shortStrkey(addr: string): string {
  if (addr.length <= 12) return addr;
  return `${addr.slice(0, 4)}…${addr.slice(-4)}`;
}

export function shortHex(bytes: Uint8Array): string {
  const head = Array.from(bytes.slice(0, 3), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
  const tail = Array.from(bytes.slice(-2), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
  return `0x${head}…${tail}`;
}
