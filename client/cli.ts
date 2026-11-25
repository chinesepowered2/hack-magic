// Round management CLI.
//   npm run round:new -- [ticks] [volatilityBps]   create + delegate a new round
//   npm run round:settle -- [market]               close (if open) and settle on Solana
//   tsx client/cli.ts status [market]              print live round state

import { equity } from "./riptide.ts";
import { RiptideClient } from "./riptide.ts";
import { loadCluster, loadKeypair, marketArg, saveRound } from "./env.ts";

const cmd = process.argv[2];
const client = new RiptideClient(loadCluster(), loadKeypair());

async function status(marketKey = marketArg(3)) {
  const m = await client.fetchMarket(marketKey, "er");
  if (!m) throw new Error("Market not found on the rollup");
  console.log(
    `Round ${m.roundId} ${m.open ? "OPEN" : "CLOSED"}  tick ${m.tick}/${m.maxTicks}  ` +
      `price ${m.price.toFixed(3)} (high ${m.high.toFixed(3)}, low ${m.low.toFixed(3)})  waves ${m.waves}`,
  );
  const traders = await client.fetchTraders(m.roster);
  traders
    .sort((a, b) => equity(b, m.price) - equity(a, m.price))
    .forEach((t, i) =>
      console.log(
        `${String(i + 1).padStart(2)}. ${t.owner.toBase58().slice(0, 8)}  ` +
          `equity ${equity(t, m.price).toFixed(2)}  size ${t.size.toFixed(3)}  trades ${t.trades}`,
      ),
    );
}

async function main() {
  switch (cmd) {
    case "new-round": {
      const ticks = Number(process.argv[3] ?? 600);
      const vol = Number(process.argv[4] ?? 40);
      const roundId = Math.floor(Date.now() / 1000);
      const market = await client.createMarket(roundId, ticks, vol);
      saveRound(market, roundId);
      console.log(`Round ${roundId} created and delegated to ${client.cluster.erRpc}`);
      console.log(`Market: ${market.toBase58()}`);
      console.log(`Open the app with ?market=${market.toBase58()}`);
      break;
    }
    case "settle": {
      const market = marketArg(3);
      const m = await client.fetchMarket(market, "er");
      if (m?.open) await client.closeRound(market);
      const sig = await client.settleMarket(market);
      console.log(`Settled on Solana: ${sig}`);
      break;
    }
    case "status":
      await status();
      break;
    default:
      console.log("usage: tsx client/cli.ts new-round [ticks] [vol] | settle [market] | status [market]");
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
