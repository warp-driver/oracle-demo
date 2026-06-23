import { FormEvent, useEffect, useState } from "react";

import {
  EthTriggerConfig,
  isMetaMaskInstalled,
  loadEthTriggerConfig,
  requestTwap as requestTwapMetaMask,
} from "../lib/metamask";
import { Asset, requestTwap as requestTwapStellar } from "../lib/oracle";

interface RequestFormProps {
  oracleId: string | null;
  walletAddress: string | null;
  /** Fires for the Freighter path: the on-chain `request_id` is known synchronously. */
  onRequest: (requestId: bigint, txHash: string) => void;
  /**
   * Fires for the MetaMask path: only the Sepolia tx hash and the
   * lowercased requester address are known yet — the parent watches
   * the `twapreq` event stream for an `originator` match before it
   * can resolve the eventual Stellar `request_id`.
   */
  onMetaMaskRequest: (requester: string, txHash: string) => void;
  onError: (message: string) => void;
}

const ASSETS: { value: Asset; label: string }[] = [
  { value: "btc_usd", label: "BTC-USD" },
  { value: "eth_usd", label: "ETH-USD" },
];

const RANGES: { value: number; label: string }[] = [
  { value: 3600, label: "1 hour" },
  { value: 21600, label: "6 hours" },
  { value: 86400, label: "24 hours" },
];

function shortTx(hash: string): string {
  if (hash.length <= 12) return hash;
  return `${hash.slice(0, 8)}…${hash.slice(-6)}`;
}

export function RequestForm({
  oracleId,
  walletAddress,
  onRequest,
  onMetaMaskRequest,
  onError,
}: RequestFormProps) {
  const [asset, setAsset] = useState<Asset>("btc_usd");
  const [rangeSecs, setRangeSecs] = useState<number>(3600);
  const [submittingFreighter, setSubmittingFreighter] = useState(false);
  const [submittingMetaMask, setSubmittingMetaMask] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const [ethConfig, setEthConfig] = useState<EthTriggerConfig | null>(null);
  const [ethConfigResolved, setEthConfigResolved] = useState(false);
  const [metaMaskPresent, setMetaMaskPresent] = useState(false);

  // Probe MetaMask + load the bridge config once on mount. Both are
  // optional: a Stellar-only demo deploy has neither.
  useEffect(() => {
    let cancelled = false;
    setMetaMaskPresent(isMetaMaskInstalled());
    void loadEthTriggerConfig().then((cfg) => {
      if (cancelled) return;
      setEthConfig(cfg);
      setEthConfigResolved(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const anySubmitting = submittingFreighter || submittingMetaMask;
  const freighterDisabled =
    !oracleId || !walletAddress || anySubmitting;
  const metaMaskDisabled =
    !ethConfig || !metaMaskPresent || anySubmitting;

  async function handleFreighterSubmit(e: FormEvent) {
    e.preventDefault();
    if (!oracleId || !walletAddress) return;
    setSubmittingFreighter(true);
    setHint("Awaiting Freighter signature…");
    try {
      const { requestId, txHash } = await requestTwapStellar({
        oracleId,
        walletAddress,
        asset,
        rangeSecs,
      });
      setHint(`request_id = ${requestId.toString()}`);
      onRequest(requestId, txHash);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setHint(null);
      onError(message);
    } finally {
      setSubmittingFreighter(false);
    }
  }

  async function handleMetaMaskSubmit() {
    if (!ethConfig) return;
    setSubmittingMetaMask(true);
    setHint("Awaiting MetaMask confirmation on Sepolia…");
    try {
      const { txHash, requester } = await requestTwapMetaMask(
        ethConfig,
        asset,
        rangeSecs,
      );
      setHint(
        `ETH tx ${shortTx(txHash)} — waiting for Warp Drive bridge…`,
      );
      onMetaMaskRequest(requester, txHash);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setHint(null);
      onError(message);
    } finally {
      setSubmittingMetaMask(false);
    }
  }

  const freighterTitle = !oracleId
    ? "Oracle contract not configured"
    : !walletAddress
      ? "Connect Freighter first"
      : "Build, sign, and submit request_twap on Stellar";

  const metaMaskTitle = !metaMaskPresent
    ? "MetaMask not detected"
    : !ethConfigResolved
      ? "Loading bridge config…"
      : !ethConfig
        ? "Bridge not configured"
        : "Submit TwapTrigger.request on Sepolia (Warp Drive bridges to Stellar)";

  return (
    <form className="section" onSubmit={handleFreighterSubmit}>
      <h2 className="section-title">Request TWAP</h2>
      <div className="form-row">
        <div className="form-group">
          <label className="form-label">Asset</label>
          <div className="radio-group" role="radiogroup">
            {ASSETS.map((opt) => (
              <label
                key={opt.value}
                className={`radio-pill ${asset === opt.value ? "checked" : ""}`}
              >
                <input
                  type="radio"
                  name="asset"
                  value={opt.value}
                  checked={asset === opt.value}
                  onChange={() => setAsset(opt.value)}
                />
                {opt.label}
              </label>
            ))}
          </div>
        </div>

        <div className="form-group">
          <label className="form-label" htmlFor="range-select">
            Range
          </label>
          <select
            id="range-select"
            className="select"
            value={rangeSecs}
            onChange={(e) => setRangeSecs(Number(e.target.value))}
          >
            {RANGES.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>

        <div
          className="form-actions"
          style={{ display: "flex", gap: 8, flexWrap: "wrap" }}
        >
          <button
            type="submit"
            className="btn btn-primary"
            disabled={freighterDisabled}
            title={freighterTitle}
          >
            {submittingFreighter
              ? "Submitting…"
              : "Request via Freighter (Stellar)"}
          </button>
          <button
            type="button"
            className="btn"
            onClick={handleMetaMaskSubmit}
            disabled={metaMaskDisabled}
            title={metaMaskTitle}
          >
            {submittingMetaMask
              ? "Submitting…"
              : "Request via MetaMask (Sepolia)"}
          </button>
        </div>
      </div>
      {hint && <p className="form-msg">{hint}</p>}
    </form>
  );
}
