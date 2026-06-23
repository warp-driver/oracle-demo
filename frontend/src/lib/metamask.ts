// MetaMask demo path — connect + sign a fixed message via
// `personal_sign`. Intentionally minimal: this demonstrates that the
// Ethereum signing primitive works in the same UI as the Stellar flow.
// No EVM contract call.

export const DEMO_MESSAGE =
  "I confirm I am the operator of this Vectr (test).";

export interface MetaMaskSignResult {
  address: string;
  signature: string;
}

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

export function isMetaMaskInstalled(): boolean {
  return getProvider() !== null;
}

function isStringArray(v: unknown): v is string[] {
  return Array.isArray(v) && v.every((x) => typeof x === "string");
}

export async function connectAndSign(): Promise<MetaMaskSignResult> {
  const provider = getProvider();
  if (!provider) {
    throw new Error("MetaMask not detected");
  }
  const accountsRaw = await provider.request({ method: "eth_requestAccounts" });
  if (!isStringArray(accountsRaw) || accountsRaw.length === 0) {
    throw new Error("MetaMask returned no accounts");
  }
  const address = accountsRaw[0];
  const signatureRaw = await provider.request({
    method: "personal_sign",
    params: [DEMO_MESSAGE, address],
  });
  if (typeof signatureRaw !== "string") {
    throw new Error("MetaMask returned a non-string signature");
  }
  return { address, signature: signatureRaw };
}
