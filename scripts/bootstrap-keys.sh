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
