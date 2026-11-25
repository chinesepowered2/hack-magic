// Demo traders so a round has company. Each bot is a fresh keypair funded by
// your wallet with a little devnet SOL for its base-layer accounts; all of its
// trading then happens on the rollup at zero cost.
//
//   npm run bots -- [market] [count]

import { Keypair, LAMPORTS_PER_SOL, SystemProgram } from "@solana/web3.js";
import { RiptideClient, STARTING_CHIPS } from "./riptide.ts";
import { loadCluster, loadKeypair, marketArg, sleep } from "./env.ts";

const cluster = loadCluster();
const funder = new RiptideClient(cluster, loadKeypair());
const market = marketArg(2);
const count = Number(process.argv[3] ?? 4);

type Style = "momentum" | "contrarian" | "random";
const STYLES: Style[] = ["momentum", "contrarian", "random"];

async function runBot(i: number) {
  const kp = Keypair.generate();
  const bot = new RiptideClient(cluster, kp);
  const style = STYLES[i % STYLES.length];
  await funder.send("base", [
    SystemProgram.transfer({
      fromPubkey: funder.me,
      toPubkey: kp.publicKey,
      lamports: 0.02 * LAMPORTS_PER_SOL,
    }),
  ]);
  await bot.join(market);
  await sleep(1500); // let the rollup clone the delegated account
  await bot.enter(market);
  console.log(`bot ${i} (${style}) ${kp.publicKey.toBase58()} entered`);

  let side = 0 as -1 | 0 | 1;
  for (;;) {
    await sleep(1500 + Math.random() * 2500);
    const m = await bot.fetchMarket(market);
    if (!m || !m.open) return;
    const h = m.history;
    const trend = h.length > 4 ? h[h.length - 1] - h[h.length - 5] : 0;
    const pick = (): -1 | 0 | 1 => {
      if (style === "momentum") return trend > 0 ? 1 : trend < 0 ? -1 : side;
      if (style === "contrarian") return trend > 0 ? -1 : trend < 0 ? 1 : side;
      return ([-1, 0, 1] as const)[Math.floor(Math.random() * 3)];
    };
    const next = pick();
    if (next === side) continue;
    const size = STARTING_CHIPS * (0.5 + Math.random() * 2);
    await bot
      .trade(market, next, next === 0 ? 0 : size)
      .then(() => (side = next))
      .catch((e) => console.warn(`bot ${i} trade failed: ${e.message}`));
  }
}

await Promise.all(Array.from({ length: count }, (_, i) => runBot(i).catch((e) => console.error(e))));
