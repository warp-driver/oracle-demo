import { FormEvent, useState } from "react";

import { Asset, requestTwap } from "../lib/oracle";

interface RequestFormProps {
  oracleId: string | null;
  walletAddress: string | null;
  onRequest: (requestId: bigint, txHash: string) => void;
  onError: (message: string) => void;
}

const ASSETS: { value: Asset; label: string }[] = [
  { value: "btc-usd", label: "BTC-USD" },
  { value: "eth-usd", label: "ETH-USD" },
];

const RANGES: { value: number; label: string }[] = [
  { value: 3600, label: "1 hour" },
  { value: 21600, label: "6 hours" },
  { value: 86400, label: "24 hours" },
];

export function RequestForm({
  oracleId,
  walletAddress,
  onRequest,
  onError,
}: RequestFormProps) {
  const [asset, setAsset] = useState<Asset>("btc-usd");
  const [rangeSecs, setRangeSecs] = useState<number>(3600);
  const [submitting, setSubmitting] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  const disabled = !oracleId || !walletAddress || submitting;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!oracleId || !walletAddress) return;
    setSubmitting(true);
    setHint("Awaiting Freighter signature…");
    try {
      const { requestId, txHash } = await requestTwap({
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
      setSubmitting(false);
    }
  }

  return (
    <form className="section" onSubmit={handleSubmit}>
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

        <div className="form-actions">
          <button
            type="submit"
            className="btn btn-primary"
            disabled={disabled}
            title={
              !oracleId
                ? "Oracle contract not configured"
                : !walletAddress
                  ? "Connect Freighter first"
                  : "Build, sign, and submit request_twap"
            }
          >
            {submitting ? "Submitting…" : "Request TWAP"}
          </button>
        </div>
      </div>
      {hint && <p className="form-msg">{hint}</p>}
    </form>
  );
}
