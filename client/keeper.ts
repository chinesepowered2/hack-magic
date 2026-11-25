// The keeper drives a round: it asks the VRF oracle for a new tide every
// RIPTIDE_TICK_MS (default 500 ms) and liquidates wiped-out traders.
// It holds no special power: request_tide and liquidate are permissionless,
// and only the VRF callback can move the price.
//
//   npm run keeper -- [market]

import { equity, RiptideClient } from "./riptide.ts";
import { loadCluster, loadKeypair, marketArg, sleep } from "./env.ts";

const client = new RiptideClient(loadCluster(), loadKeypair());
const market = marketArg(2);
const tickMs = Number(process.env.RIPTIDE_TICK_MS ?? 500);

async function liquidations() {
  const m = await client.fetchMarket(market);
  if (!m) return;
  const traders = await client.fetchTraders(m.roster);
  for (const t of traders) {
    if (t.size !== 0 && equity(t, m.price) <= 0) {
      await client
        .liquidate(market, t.address)
        .then(() => console.log(`liquidated ${t.owner.toBase58()}`))
        .catch((e) => console.warn(`liquidate failed: ${e.message}`));
    }
  }
}

async function main() {
  console.log(`Keeper on ${client.cluster.erRpc} for market ${market.toBase58()}`);
  let seed = 0;
  let lastTick = -1;
  let inFlight = 0;
  for (;;) {
    const m = await client.fetchMarket(market).catch(() => null);
    if (!m) {
      console.log("market not visible on the rollup yet");
      await sleep(1000);
      continue;
    }
    if (!m.open) {
      console.log(`Round closed at tick ${m.tick}, price ${m.price.toFixed(3)}. Run npm run round:settle.`);
      return;
    }
    if (m.tick !== lastTick) {
      lastTick = m.tick;
      const wave = m.lastWave === 1 ? "  WAVE UP" : m.lastWave === -1 ? "  WAVE DOWN" : "";
      console.log(`tick ${m.tick}/${m.maxTicks} price ${m.price.toFixed(3)}${wave}`);
      if (m.tick % 5 === 0) liquidations().catch(() => {});
    }
    // Keep at most two randomness requests in flight.
    if (inFlight < 2) {
      inFlight++;
      client
        .requestTide(market, seed++)
        .catch((e) => console.warn(`request_tide failed: ${e.message}`))
        .finally(() => inFlight--);
    }
    await sleep(tickMs);
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
