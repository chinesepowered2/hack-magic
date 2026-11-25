// Node-side helpers for the CLI, keeper and bots.

import { Keypair, PublicKey } from "@solana/web3.js";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { CLUSTERS, type Cluster } from "./riptide.ts";

export function loadKeypair(file = process.env.RIPTIDE_KEYPAIR): Keypair {
  const p = file ?? path.join(os.homedir(), ".config", "solana", "id.json");
  const bytes = JSON.parse(fs.readFileSync(p.replace(/^~/, os.homedir()), "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(bytes));
}

export function loadCluster(): Cluster {
  const name = process.env.RIPTIDE_CLUSTER ?? "devnet";
  const base = CLUSTERS[name];
  if (!base) throw new Error(`Unknown RIPTIDE_CLUSTER ${name}`);
  return {
    ...base,
    baseRpc: process.env.RIPTIDE_BASE_RPC ?? base.baseRpc,
    erRpc: process.env.RIPTIDE_ER_RPC ?? base.erRpc,
    erValidator: process.env.RIPTIDE_ER_VALIDATOR
      ? new PublicKey(process.env.RIPTIDE_ER_VALIDATOR)
      : base.erValidator,
    oracleQueue: process.env.RIPTIDE_ORACLE_QUEUE
      ? new PublicKey(process.env.RIPTIDE_ORACLE_QUEUE)
      : base.oracleQueue,
  };
}

const ROUND_FILE = path.resolve(".riptide-round.json");

export function saveRound(market: PublicKey, roundId: number) {
  fs.writeFileSync(ROUND_FILE, JSON.stringify({ market: market.toBase58(), roundId }, null, 2));
}

/** Market from argv[pos], RIPTIDE_MARKET, or the last round this machine created. */
export function marketArg(pos: number): PublicKey {
  const arg = process.argv[pos] ?? process.env.RIPTIDE_MARKET;
  if (arg) return new PublicKey(arg);
  if (fs.existsSync(ROUND_FILE)) {
    return new PublicKey(JSON.parse(fs.readFileSync(ROUND_FILE, "utf8")).market);
  }
  throw new Error("No market given. Pass one, set RIPTIDE_MARKET, or run `npm run round:new`.");
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
