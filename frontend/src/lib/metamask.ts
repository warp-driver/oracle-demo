// MetaMask Sepolia path. Replaces the previous `personal_sign` demo
// with an actual call to the `TwapTrigger.request(string,uint32)`
// contract on Sepolia, which the Warp Drive node bridges to a
// Stellar `request_twap` via the `bridge_eth_request` workflow.
//
// Everything here is hand-rolled — no ethers/viem dependency. The
// surface is small (one calldata shape, one chain switch) and a thin
// wrapper around `window.ethereum.request` keeps the bundle lean.

export interface MetaMaskRequestResult {
  /** 0x-prefixed Sepolia tx hash. */
  txHash: string;
  /** Lowercase 0x-prefixed hex, 42 chars. The Sepolia `msg.sender`. */
  requester: string;
  /** Decoded block number from the receipt. */
  blockNumber: number;
}

export interface EthTriggerConfig {
  /** 0x-prefixed contract address. */
  address: string;
  /** Decimal chain id string, e.g. "11155111". */
  chain_id: string;
  /** 64 hex chars, no 0x prefix. The Sepolia event topic0. */
  event_hash: string;
}

// ─── constants ───────────────────────────────────────────────────────

const ETH_TRIGGER_JSON_URL = "/eth-trigger.json";

/** Sepolia = 11155111 = 0xaa36a7. */
const SEPOLIA_CHAIN_ID_HEX = "0xaa36a7";

const SEPOLIA_RPC_URL = "https://ethereum-sepolia-rpc.publicnode.com";

/**
 * Function selector for `request(string,uint32)`. First 4 bytes of
 * `keccak256("request(string,uint32)")`. Verified via
 * `cast sig 'request(string,uint32)'` = `0xd4f0eb50`.
 */
const REQUEST_SELECTOR_HEX = "d4f0eb50";

/** MetaMask error code for "chain not added to the wallet". */
const ERR_UNRECOGNISED_CHAIN = 4902;

/** Receipt poll: 250ms intervals, 60s budget. */
const RECEIPT_POLL_INTERVAL_MS = 250;
const RECEIPT_POLL_TIMEOUT_MS = 60_000;

// ─── provider plumbing ──────────────────────────────────────────────

interface EthereumProvider {
  isMetaMask?: boolean;
  request(args: { method: string; params?: unknown[] }): Promise<unknown>;
}

function getProvider(): EthereumProvider | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as { ethereum?: unknown };
  const eth = w.ethereum;
  if (!eth || typeof eth !== "object") return null;
  const candidate = eth as Record<string, unknown>;
  if (typeof candidate.request !== "function") return null;
  return candidate as unknown as EthereumProvider;
}

function getProviderOrThrow(): EthereumProvider {
  const p = getProvider();
  if (!p) throw new Error("MetaMask not detected");
  return p;
}

export function isMetaMaskInstalled(): boolean {
  return getProvider() !== null;
}

// ─── tiny type guards ────────────────────────────────────────────────

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function isStringArray(v: unknown): v is string[] {
  return Array.isArray(v) && v.every((x) => typeof x === "string");
}

function isEthTriggerConfig(v: unknown): v is EthTriggerConfig {
  if (!isRecord(v)) return false;
  return (
    typeof v.address === "string" &&
    typeof v.chain_id === "string" &&
    typeof v.event_hash === "string"
  );
}

function errorCode(err: unknown): number | undefined {
  if (!isRecord(err)) return undefined;
  return typeof err.code === "number" ? err.code : undefined;
}

// ─── config loader ───────────────────────────────────────────────────

/**
 * Mirrors `App.tsx`'s `loadOracleId`: fetch the bridge config JSON
 * written by `task frontend-config`. Returns `null` if absent so the
 * caller can render a "Bridge not configured" state gracefully without
 * throwing — most demo deploys won't include the Sepolia bridge.
 */
export async function loadEthTriggerConfig(): Promise<EthTriggerConfig | null> {
  try {
    const resp = await fetch(ETH_TRIGGER_JSON_URL, { cache: "no-cache" });
    if (!resp.ok) return null;
    const ctype = resp.headers.get("content-type") ?? "";
    if (!ctype.includes("application/json")) return null;
    const json: unknown = await resp.json();
    if (!isEthTriggerConfig(json)) return null;
    return json;
  } catch {
    return null;
  }
}

// ─── network switch ──────────────────────────────────────────────────

/**
 * Ensure the active wallet network is Sepolia. If the chain isn't
 * known to MetaMask (error code 4902), add it via
 * `wallet_addEthereumChain` — MetaMask auto-switches as part of that
 * flow, so no follow-up switch call is required.
 */
export async function ensureSepolia(): Promise<void> {
  const provider = getProviderOrThrow();
  try {
    await provider.request({
      method: "wallet_switchEthereumChain",
      params: [{ chainId: SEPOLIA_CHAIN_ID_HEX }],
    });
  } catch (err) {
    if (errorCode(err) !== ERR_UNRECOGNISED_CHAIN) throw err;
    await provider.request({
      method: "wallet_addEthereumChain",
      params: [
        {
          chainId: SEPOLIA_CHAIN_ID_HEX,
          chainName: "Sepolia",
          rpcUrls: [SEPOLIA_RPC_URL],
          nativeCurrency: { name: "Sepolia Ether", symbol: "ETH", decimals: 18 },
          blockExplorerUrls: ["https://sepolia.etherscan.io"],
        },
      ],
    });
  }
}

/**
 * Convenience used by the Header connect button: switch to Sepolia,
 * grab the first authorised account, return it lowercased.
 */
export async function connectSepolia(): Promise<string> {
  await ensureSepolia();
  const provider = getProviderOrThrow();
  const accountsRaw = await provider.request({ method: "eth_requestAccounts" });
  if (!isStringArray(accountsRaw) || accountsRaw.length === 0) {
    throw new Error("MetaMask returned no accounts");
  }
  return accountsRaw[0].toLowerCase();
}

// ─── calldata encoder ────────────────────────────────────────────────

/**
 * ABI-encode `(string asset, uint32 rangeSecs)` for
 * `TwapTrigger.request`. Layout:
 *
 *   head[0]   offset of dynamic string = 0x40 (2 head words)
 *   head[1]   rangeSecs, right-aligned in 32-byte word
 *   tail[0]   string length L (u256)
 *   tail[1..] UTF-8 bytes, zero-padded to a 32-byte multiple
 */
function encodeCalldata(asset: string, rangeSecs: number): string {
  const stringOffset =
    "0000000000000000000000000000000000000000000000000000000000000040";
  const rangeHex = (rangeSecs >>> 0).toString(16).padStart(64, "0");
  const utf8 = new TextEncoder().encode(asset);
  const lenHex = utf8.length.toString(16).padStart(64, "0");
  let bodyHex = "";
  for (const b of utf8) bodyHex += b.toString(16).padStart(2, "0");
  const remainder = utf8.length % 32;
  const padBytes = remainder === 0 ? 0 : 32 - remainder;
  const padHex = "00".repeat(padBytes);
  return (
    "0x" + REQUEST_SELECTOR_HEX + stringOffset + rangeHex + lenHex + bodyHex + padHex
  );
}

// ─── receipt poll ────────────────────────────────────────────────────

interface TxReceipt {
  status: string;
  blockNumber: string;
  transactionHash: string;
}

function isTxReceipt(v: unknown): v is TxReceipt {
  if (!isRecord(v)) return false;
  return (
    typeof v.status === "string" &&
    typeof v.blockNumber === "string" &&
    typeof v.transactionHash === "string"
  );
}

// `Promise.withResolvers` is ES2024; the project's tsconfig.json
// declares `lib: ["ES2022", ...]`, so the static type isn't in scope.
// All evergreen browsers (Chrome 119+, Firefox 121+, Safari 17.4+)
// ship it at runtime — augment the type locally rather than widening
// the project-wide lib floor.
declare global {
  interface PromiseConstructor {
    withResolvers<T>(): {
      promise: Promise<T>;
      resolve: (value: T | PromiseLike<T>) => void;
      reject: (reason?: unknown) => void;
    };
  }
}

function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}

async function pollReceipt(
  provider: EthereumProvider,
  txHash: string,
): Promise<TxReceipt> {
  const deadline = Date.now() + RECEIPT_POLL_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const raw: unknown = await provider.request({
      method: "eth_getTransactionReceipt",
      params: [txHash],
    });
    if (raw !== null && raw !== undefined) {
      if (!isTxReceipt(raw)) {
        throw new Error("eth_getTransactionReceipt returned unexpected shape");
      }
      return raw;
    }
    await delay(RECEIPT_POLL_INTERVAL_MS);
  }
  throw new Error(
    `Sepolia tx ${txHash} not mined within ${RECEIPT_POLL_TIMEOUT_MS / 1000}s`,
  );
}

// ─── public entry: requestTwap ───────────────────────────────────────

/**
 * Submit a `TwapTrigger.request(asset, rangeSecs)` transaction on
 * Sepolia and wait for inclusion. The Warp Drive bridge workflow will
 * observe the resulting `TwapRequested` log and produce a matching
 * Stellar `request_twap`, surfaced to the UI through the existing
 * `twapreq` event poller.
 */
export async function requestTwap(
  config: EthTriggerConfig,
  asset: "btc_usd" | "eth_usd",
  rangeSecs: number,
): Promise<MetaMaskRequestResult> {
  await ensureSepolia();
  const provider = getProviderOrThrow();

  const accountsRaw = await provider.request({ method: "eth_requestAccounts" });
  if (!isStringArray(accountsRaw) || accountsRaw.length === 0) {
    throw new Error("MetaMask returned no accounts");
  }
  const requester = accountsRaw[0];

  const data = encodeCalldata(asset, rangeSecs);
  const sendRaw: unknown = await provider.request({
    method: "eth_sendTransaction",
    params: [{ from: requester, to: config.address, data }],
  });
  if (typeof sendRaw !== "string") {
    throw new Error("eth_sendTransaction did not return a tx hash");
  }
  const txHash = sendRaw;

  const receipt = await pollReceipt(provider, txHash);
  if (receipt.status !== "0x1") {
    throw new Error(`Sepolia tx ${txHash} reverted (status ${receipt.status})`);
  }
  return {
    txHash,
    requester: requester.toLowerCase(),
    blockNumber: parseInt(receipt.blockNumber, 16),
  };
}
