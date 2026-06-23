import { useEffect, useState } from "react";

import {
  connect as freighterConnect,
  isFreighterInstalled,
} from "../lib/freighter";
import { shortStrkey } from "../lib/oracle";
import {
  DEMO_MESSAGE,
  connectAndSign as connectMetaMaskAndSign,
  isMetaMaskInstalled,
} from "../lib/metamask";

interface HeaderProps {
  walletAddress: string | null;
  onWalletConnected: (address: string) => void;
  onError: (message: string) => void;
}

interface MetaMaskState {
  address: string;
  signature: string;
}

export function Header({
  walletAddress,
  onWalletConnected,
  onError,
}: HeaderProps) {
  const [connecting, setConnecting] = useState(false);
  const [mmConnecting, setMmConnecting] = useState(false);
  const [mm, setMm] = useState<MetaMaskState | null>(null);
  const [freighterPresent, setFreighterPresent] = useState(false);
  const [metaMaskPresent, setMetaMaskPresent] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void isFreighterInstalled().then((ok) => {
      if (!cancelled) setFreighterPresent(ok);
    });
    setMetaMaskPresent(isMetaMaskInstalled());
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleFreighterClick() {
    if (walletAddress) return;
    setConnecting(true);
    try {
      const { address } = await freighterConnect();
      onWalletConnected(address);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setConnecting(false);
    }
  }

  async function handleMetaMaskClick() {
    setMmConnecting(true);
    try {
      const result = await connectMetaMaskAndSign();
      setMm(result);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setMmConnecting(false);
    }
  }

  return (
    <header className="header">
      <div className="header-titles">
        <h1>Warp Drive Oracle</h1>
        <p>BTC/USD · ETH/USD multi-round TWAP</p>
        {mm && (
          <div className="metamask-sig">
            <div>{shortStrkey(mm.address)} signed:</div>
            <div>{DEMO_MESSAGE}</div>
            <div>{mm.signature}</div>
          </div>
        )}
      </div>
      <div className="header-actions">
        {walletAddress ? (
          <button type="button" className="btn btn-connected" disabled>
            {shortStrkey(walletAddress)} ✓
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleFreighterClick}
            disabled={connecting || !freighterPresent}
            title={
              freighterPresent
                ? "Authorise Freighter to sign on testnet"
                : "Freighter extension not detected"
            }
          >
            {connecting ? "Connecting…" : "Connect Freighter"}
          </button>
        )}
        <button
          type="button"
          className="btn"
          onClick={handleMetaMaskClick}
          disabled={mmConnecting || !metaMaskPresent}
          title={
            metaMaskPresent
              ? "Connect MetaMask and sign the demo message"
              : "MetaMask not detected"
          }
        >
          {mmConnecting ? "Signing…" : mm ? "MetaMask ✓" : "Connect MetaMask"}
        </button>
      </div>
    </header>
  );
}
