#!/usr/bin/env bash
#
# bootstrap-keys.sh — generate the stellar identities the oracle-demo
# quickstart needs and print them as a .env-compatible block on stdout.
#
# Usage:
#   ./scripts/bootstrap-keys.sh > .env
#   set -a; source .env; set +a
#
# Default quickstart runs TWO operators on the same host (see README §
# Multi-operator deployment), so we mint two BIP39 mnemonics in addition
# to the funded testnet deployer key. Each operator's warpdrive node
# uses its own mnemonic via WARPDRIVE_SIGNING_MNEMONIC (op 1) or
# WARPDRIVE_SIGNING_MNEMONIC_2 (op 2). Bumping the OPERATORS env var
# adds more here — keep the names in lock-step with the Taskfile.
#
# This wipes any prior identities under these names in the local
# `stellar keys` store; they are demo-only and produced fresh on every
# run.

set -euo pipefail

OPERATORS="${OPERATORS:-2}"

stellar keys rm oracle-deployer 2>/dev/null || true
stellar keys generate oracle-deployer --fund --network testnet

DEPLOYER_SECRET=$(stellar keys show oracle-deployer)
DEPLOYER_ADDRESS=$(stellar keys address oracle-deployer)

{
    echo "DEPLOYER_SECRET=$DEPLOYER_SECRET"
    echo "DEPLOYER_ADDRESS=$DEPLOYER_ADDRESS"
    echo "OPERATORS=$OPERATORS"
} 

for op in $(seq 1 "$OPERATORS"); do
    name="warpdrive-operator-$op"
    stellar keys rm "$name" 2>/dev/null || true
    stellar keys generate "$name"
    phrase=$(stellar keys show "$name" --phrase)
    suffix=""
    [ "$op" -gt 1 ] && suffix="_$op"
    echo "WARPDRIVE_SIGNING_MNEMONIC${suffix}=\"$phrase\""
done

# CoinGecko Demo key (free, ~30 req/min after sign-up at
# https://www.coingecko.com/en/api/pricing). Without it the cron-circuit
# uses the unauthenticated public endpoint, which two operators sharing a
# single IP will rate-limit (HTTP 429) within minutes. The build-service
# step bakes whatever value is here into service.json; an empty value
# falls back to the no-key path. Set COINGECKO_API_KEY before invoking
# this script to persist your key, or edit the .env file directly later
# and re-run `task build-service && OP=N task register-service` for each
# operator to push it into the live dispatcher.
if [ -n "${COINGECKO_API_KEY:-}" ]; then
    echo "COINGECKO_API_KEY=$COINGECKO_API_KEY"
else
    echo "# COINGECKO_API_KEY=CG-xxxxxxxxxxxxxxxx   # demo key — uncomment to lift the public 429 cap"
fi

# Sepolia bridge — only needed if you want the MetaMask request path
# (see README § Triggering via MetaMask). The deployer key is used ONCE
# by `task deploy-eth-trigger` to deploy contracts/eth-trigger/TwapTrigger.sol;
# regular MetaMask users do NOT need this key, just Sepolia ETH in their
# own wallet.
if [ -n "${SEPOLIA_RPC_URL:-}" ]; then
    echo "SEPOLIA_RPC_URL=$SEPOLIA_RPC_URL"
else
    echo "# SEPOLIA_RPC_URL=https://ethereum-sepolia-rpc.publicnode.com"
fi
if [ -n "${SEPOLIA_DEPLOYER_KEY:-}" ]; then
    echo "SEPOLIA_DEPLOYER_KEY=$SEPOLIA_DEPLOYER_KEY"
else
    echo "# SEPOLIA_DEPLOYER_KEY=0x_funded_secp256k1_secret   # uncomment + fund via https://sepoliafaucet.com"
fi
