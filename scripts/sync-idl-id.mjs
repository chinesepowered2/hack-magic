#!/usr/bin/env node
// Sync the program ID everywhere the deploy script's sed cannot reach: the IDL
// stores the buffer PDA's seeds::program as a 32-byte array, not a string.
// `anchor idl build` regenerates this, but the script can run without the
// anchor CLI, so patch the committed IDL in place.
//
//   node scripts/sync-idl-id.mjs <newProgramId>

import fs from "node:fs";
import path from "node:path";
import { PublicKey } from "@solana/web3.js";

const newId = process.argv[2];
if (!newId) {
  console.error("usage: node scripts/sync-idl-id.mjs <newProgramId>");
  process.exit(1);
}
const idlPath = path.resolve("client/idl/riptide.json");
const idl = JSON.parse(fs.readFileSync(idlPath, "utf8"));

const newBytes = Array.from(new PublicKey(newId).toBuffer());
let patched = 0;
const isProgramIdBytes = (v) =>
  Array.isArray(v) &&
  v.length === 32 &&
  v.every((x) => typeof x === "number") &&
  (() => {
    try {
      // A seeds::program const under our own program equals the declared ID.
      return new PublicKey(Buffer.from(v)).toBase58() === idl.address;
    } catch {
      return false;
    }
  })();

const walk = (o) => {
  if (!o || typeof o !== "object") return;
  if (Array.isArray(o)) {
    for (let i = 0; i < o.length; i++) {
      if (isProgramIdBytes(o[i])) {
        o[i] = newBytes.slice();
        patched++;
      } else walk(o[i]);
    }
    return;
  }
  for (const k of Object.keys(o)) {
    if (isProgramIdBytes(o[k])) {
      o[k] = newBytes.slice();
      patched++;
    } else walk(o[k]);
  }
};
walk(idl);
idl.address = newId;
fs.writeFileSync(idlPath, JSON.stringify(idl, null, 2) + "\n");
console.log(`synced IDL address to ${newId} (${patched} seeds::program byte arrays patched)`);
