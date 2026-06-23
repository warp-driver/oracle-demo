// Thin wrapper around `@stellar/freighter-api`. The Freighter API
// returns `{ ... error?: FreighterApiError }` on every call rather than
// throwing — we normalise to a thrown `Error` so callers can use plain
// `try/catch`. The exported `signTransaction` keeps the signature
// expected by `lib/oracle.ts`: `(xdr, { network }) => { signedTxXdr }`.

import {
  isConnected,
  requestAccess,
  getAddress,
  signTransaction as freighterSignTx,
} from "@stellar/freighter-api";

export interface ConnectResult {
  address: string;
}

export async function isFreighterInstalled(): Promise<boolean> {
  try {
    const res = await isConnected();
    if (res.error) return false;
    return res.isConnected;
  } catch {
    return false;
  }
}

/** Prompts the Freighter popup if needed; returns the user's G-address. */
export async function connect(): Promise<ConnectResult> {
  const res = await requestAccess();
  if (res.error) {
    throw new Error(res.error.message || "Freighter access denied");
  }
  if (!res.address) {
    throw new Error("Freighter returned no address");
  }
  return { address: res.address };
}

/** Returns the currently-authorised address, or null if none. */
export async function currentAddress(): Promise<string | null> {
  try {
    const res = await getAddress();
    if (res.error || !res.address) return null;
    return res.address;
  } catch {
    return null;
  }
}

/**
 * Sign an XDR-encoded transaction. The `network` argument is the
 * passphrase (e.g. `Networks.TESTNET`); the wrapper forwards it to
 * Freighter as `networkPassphrase`.
 */
export async function signTransaction(
  xdr: string,
  opts: { network: string; address?: string },
): Promise<{ signedTxXdr: string }> {
  const res = await freighterSignTx(xdr, {
    networkPassphrase: opts.network,
    address: opts.address,
  });
  if (res.error) {
    throw new Error(res.error.message || "Freighter signing failed");
  }
  return { signedTxXdr: res.signedTxXdr };
}
