import { useEffect, useState } from "react";

import {
  connect as freighterConnect,
  isFreighterInstalled,
} from "../lib/freighter";
import { shortStrkey } from "../lib/oracle";
import {
  connectSepolia,
  isMetaMaskInstalled,
} from "../lib/metamask";

interface HeaderProps {
  walletAddress: string | null;
  onWalletConnected: (address: string) => void;
  onError: (message: string) => void;
}

function shortEth(addr: string): string {
  if (addr.length <= 12) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

export function Header({
  walletAddress,
  onWalletConnected,
  onError,
}: HeaderProps) {
  const [connecting, setConnecting] = useState(false);
  const [mmConnecting, setMmConnecting] = useState(false);
  const [mmAddress, setMmAddress] = useState<string | null>(null);
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
    if (mmAddress) return;
    setMmConnecting(true);
    try {
      const address = await connectSepolia();
      setMmAddress(address);
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
        {mmAddress ? (
          <button type="button" className="btn btn-connected" disabled>
            {shortEth(mmAddress)} ✓
          </button>
        ) : (
          <button
            type="button"
            className="btn"
            onClick={handleMetaMaskClick}
            disabled={mmConnecting || !metaMaskPresent}
            title={
              metaMaskPresent
                ? "Switch MetaMask to Sepolia and authorise the account"
                : "MetaMask not detected"
            }
          >
            {mmConnecting ? "Connecting…" : "Connect to Sepolia"}
          </button>
        )}
      </div>
    </header>
  );
}
