#!/usr/bin/env bash
# Deploy Riptide to Solana devnet and open a round on MagicBlock's devnet rollup.
#
#   ./scripts/deploy-devnet.sh [ticks=600] [volatilityBps=40]
#
# Needs: Solana CLI (with cargo build-sbf), Anchor CLI 1.x, Node 20+, and a
# devnet wallet at ~/.config/solana/id.json (or $RIPTIDE_KEYPAIR) holding
# about 4 devnet SOL for the program deploy.
#
# The program keypair is read from keys/riptide-keypair.json (git-ignored).
# If it isn't there, a fresh one is generated and the program ID is synced
# into lib.rs, Anchor.toml and the IDL before building.
set -euo pipefail
cd "$(dirname "$0")/.."

TICKS="${1:-600}"
VOL="${2:-40}"
WALLET="${RIPTIDE_KEYPAIR:-$HOME/.config/solana/id.json}"
PROGRAM_KP="keys/riptide-keypair.json"
URL="${RIPTIDE_BASE_RPC:-https://api.devnet.solana.com}"

mkdir -p keys
if [ ! -f "$PROGRAM_KP" ]; then
  echo "No $PROGRAM_KP found, generating a new program keypair"
  solana-keygen new --no-bip39-passphrase --silent -o "$PROGRAM_KP"
fi

NEW_ID="$(solana-keygen pubkey "$PROGRAM_KP")"
OLD_ID="$(grep -oE 'declare_id!\("[1-9A-HJ-NP-Za-km-z]+"\)' programs/riptide/src/lib.rs | sed -E 's/declare_id!\("(.*)"\)/\1/')"
if [ "$NEW_ID" != "$OLD_ID" ]; then
  echo "Syncing program ID $OLD_ID -> $NEW_ID"
  sed -i.bak "s/$OLD_ID/$NEW_ID/g" programs/riptide/src/lib.rs Anchor.toml && rm -f programs/riptide/src/lib.rs.bak Anchor.toml.bak
fi

echo "Building the program and IDL"
npm install --silent
cargo build-sbf --manifest-path programs/riptide/Cargo.toml
if command -v anchor >/dev/null 2>&1; then
  anchor idl build -p riptide -o target/idl/riptide.json -t target/types/riptide.ts
  cp target/idl/riptide.json client/idl/riptide.json
fi
# The IDL also carries the program ID as raw bytes (the delegation buffer PDA's
# seeds::program), which sed cannot reach; sync it in place.
node scripts/sync-idl-id.mjs "$NEW_ID"

echo "Wallet $(solana-keygen pubkey "$WALLET") has $(solana balance -u "$URL" -k "$WALLET")"
echo "Deploying $NEW_ID to devnet"
solana program deploy target/deploy/riptide.so \
  --program-id "$PROGRAM_KP" -k "$WALLET" -u "$URL"

echo "Opening a $TICKS-tick round (volatility $VOL bps) on the rollup"
RIPTIDE_KEYPAIR="$WALLET" npm run --silent round:new -- "$TICKS" "$VOL"

MARKET="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(".riptide-round.json","utf8")).market)')"
cat <<MSG

Riptide is live on devnet.
  Program: $NEW_ID
  Market:  $MARKET

Next, in separate terminals:
  npm run keeper                 # moves the tide every 500 ms
  npm run bots -- "" 4           # optional demo traders
  npm run dev                    # then open http://localhost:5173/?market=$MARKET
MSG
