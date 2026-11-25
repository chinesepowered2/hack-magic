import { Keypair, LAMPORTS_PER_SOL, PublicKey } from "@solana/web3.js";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CLUSTERS,
  MAX_LEVERAGE,
  PROGRAM_ID,
  RiptideClient,
  equity,
  traderPda,
  unrealized,
  type MarketState,
  type TraderState,
} from "../../client/riptide.ts";
import { Chart } from "./Chart.tsx";

const params = new URLSearchParams(window.location.search);
const clusterName = params.get("cluster") ?? import.meta.env.VITE_CLUSTER ?? "devnet";
const baseCluster = CLUSTERS[clusterName] ?? CLUSTERS.devnet;
const cluster = {
  ...baseCluster,
  baseRpc: import.meta.env.VITE_BASE_RPC ?? baseCluster.baseRpc,
  erRpc: import.meta.env.VITE_ER_RPC ?? baseCluster.erRpc,
};
const marketParam = params.get("market") ?? import.meta.env.VITE_MARKET ?? "";

function loadBurner(): Keypair {
  const key = `riptide-burner-${cluster.name}`;
  try {
    const stored = localStorage.getItem(key);
    if (stored) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(stored)));
  } catch {
    /* fall through to a fresh key */
  }
  const kp = Keypair.generate();
  try {
    localStorage.setItem(key, JSON.stringify(Array.from(kp.secretKey)));
  } catch {
    /* private mode: the key lives for this tab only */
  }
  return kp;
}

const short = (k: PublicKey | string) => {
  const s = typeof k === "string" ? k : k.toBase58();
  return `${s.slice(0, 4)}…${s.slice(-4)}`;
};
const fmt = (n: number, d = 2) =>
  n.toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d });
const hex = (b: Uint8Array) => Array.from(b.slice(0, 8), (x) => x.toString(16).padStart(2, "0")).join("");

interface Toast {
  id: number;
  text: string;
  kind: "ok" | "err" | "wave";
}

export function App() {
  const burner = useMemo(loadBurner, []);
  const client = useMemo(() => new RiptideClient(cluster, burner), [burner]);
  const market = useMemo(() => {
    try {
      return marketParam ? new PublicKey(marketParam) : null;
    } catch {
      return null;
    }
  }, []);

  const [m, setM] = useState<MarketState | null>(null);
  const [me, setMe] = useState<TraderState | null>(null);
  const [board, setBoard] = useState<TraderState[]>([]);
  const [sol, setSol] = useState<number | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [size, setSize] = useState(2000);
  const [latency, setLatency] = useState<number | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [waves, setWaves] = useState<{ tick: number; dir: number }[]>([]);
  const lastTick = useRef(-1);
  const toastId = useRef(0);

  const toast = useCallback((text: string, kind: Toast["kind"] = "ok") => {
    const id = ++toastId.current;
    setToasts((t) => [...t.slice(-3), { id, text, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4000);
  }, []);

  const onMarket = useCallback(
    (next: MarketState) => {
      setM(next);
      if (next.tick !== lastTick.current) {
        if (lastTick.current >= 0 && next.lastWave !== 0) {
          setWaves((w) => [...w.slice(-20), { tick: next.tick, dir: next.lastWave }]);
          toast(next.lastWave > 0 ? "RIPTIDE: the tide surges up" : "RIPTIDE: the tide rips down", "wave");
        }
        lastTick.current = next.tick;
      }
    },
    [toast],
  );

  // Live market: websocket from the rollup, with a polling fallback.
  useEffect(() => {
    if (!market) return;
    let alive = true;
    const unsub = client.onMarket(market, (x) => alive && onMarket(x));
    const poll = async () => {
      const x = await client.fetchMarket(market).catch(() => null);
      if (alive && x) onMarket(x);
    };
    poll();
    const t = setInterval(poll, 1000);
    return () => {
      alive = false;
      unsub();
      clearInterval(t);
    };
  }, [client, market, onMarket]);

  // My trader account and the leaderboard.
  useEffect(() => {
    if (!market) return;
    let alive = true;
    const mine = traderPda(market, burner.publicKey);
    const poll = async () => {
      const roster = m?.roster ?? [];
      const keys = roster.some((k) => k.equals(mine)) ? roster : [...roster, mine];
      const traders = await client.fetchTraders(keys).catch(() => null);
      if (!alive || !traders) return;
      setMe(traders.find((t) => t.address.equals(mine)) ?? null);
      setBoard(traders.filter((t) => roster.some((k) => k.equals(t.address))));
    };
    poll();
    const t = setInterval(poll, 1000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [client, market, burner, m?.roster.length]);

  // Base-layer SOL for joining.
  useEffect(() => {
    let alive = true;
    const poll = () =>
      client.base
        .getBalance(burner.publicKey)
        .then((b) => alive && setSol(b / LAMPORTS_PER_SOL))
        .catch(() => {});
    poll();
    const t = setInterval(poll, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [client, burner]);

  const run = useCallback(
    async (label: string, fn: () => Promise<unknown>) => {
      setBusy(label);
      try {
        await fn();
      } catch (e: any) {
        console.error(e);
        toast(`${label} failed: ${e?.message ?? e}`.slice(0, 160), "err");
      } finally {
        setBusy(null);
      }
    },
    [toast],
  );

  const fund = () =>
    run("Airdrop", async () => {
      const sig = await client.base.requestAirdrop(burner.publicKey, LAMPORTS_PER_SOL);
      await client.base.confirmTransaction(sig, "confirmed");
      toast("1 devnet SOL received");
    });

  const join = () =>
    run("Join", async () => {
      await client.join(market!);
      // The rollup clones the freshly delegated account on first use.
      for (let attempt = 0; ; attempt++) {
        try {
          await client.enter(market!);
          break;
        } catch (e) {
          if (attempt >= 6) throw e;
          await new Promise((r) => setTimeout(r, 1000));
        }
      }
      toast("You're in. Trades are now free and instant.");
    });

  const trade = (dir: -1 | 0 | 1) =>
    run(dir === 0 ? "Flatten" : dir > 0 ? "Long" : "Short", async () => {
      const t0 = performance.now();
      await client.trade(market!, dir, dir === 0 ? 0 : size);
      setLatency(Math.round(performance.now() - t0));
    });

  const cashOut = () =>
    run("Cash out", async () => {
      await client.cashOut(market!);
      toast("Cashed out. Your final chips are settled on Solana.");
    });

  if (!market) return <Landing />;

  const price = m?.price ?? 100;
  const change = m ? (m.price / m.openPrice - 1) * 100 : 0;
  const myEq = me ? equity(me, price) : 0;
  const myPnl = me ? unrealized(me, price) : 0;
  const ranked = [...board].sort((a, b) => equity(b, price) - equity(a, price));
  const historyLen = m?.history.length ?? 0;
  const waveDots = m
    ? waves.map((w) => ({ index: historyLen - 1 - (m.tick - w.tick), dir: w.dir }))
    : [];
  const progress = m ? Math.min(100, (m.tick / m.maxTicks) * 100) : 0;
  const maxSize = me ? Math.max(0, Math.floor(me.cash * MAX_LEVERAGE)) : 50_000;
  const inRound = !!me?.entered && !me.cashedOut;

  return (
    <div className="shell">
      <header className="top">
        <div className="brand">
          <img src="/favicon.svg" alt="" width={28} height={28} />
          <span>Riptide</span>
        </div>
        <div className="round">
          {m ? (
            <>
              <span className={m.open ? "pill live" : "pill closed"}>{m.open ? "LIVE" : "SETTLING"}</span>
              <span>Round {m.roundId}</span>
              <span className="muted">
                tick {m.tick}/{m.maxTicks}
              </span>
            </>
          ) : (
            <span className="muted">connecting to the rollup…</span>
          )}
        </div>
        <div className="net muted">{cluster.name} · {short(PROGRAM_ID)}</div>
      </header>
      <div className="progress">
        <div style={{ width: `${progress}%` }} />
      </div>

      <main className="grid">
        <section className="card market">
          <div className="price-row">
            <div>
              <div className="label">TIDE PRICE</div>
              <div className="price">{fmt(price, 3)}</div>
            </div>
            <div className={change >= 0 ? "delta up" : "delta down"}>
              {change >= 0 ? "▲" : "▼"} {fmt(Math.abs(change))}%
            </div>
            <div className="stats">
              <div>
                <span className="label">HIGH</span> {fmt(m?.high ?? 100, 2)}
              </div>
              <div>
                <span className="label">LOW</span> {fmt(m?.low ?? 100, 2)}
              </div>
              <div>
                <span className="label">WAVES</span> {m?.waves ?? 0}
              </div>
            </div>
          </div>
          <Chart
            prices={m?.history ?? []}
            openPrice={m?.openPrice ?? 100}
            entry={me && me.size !== 0 ? me.entryPrice : undefined}
            side={me?.size}
            waves={waveDots}
          />
          <div className="vrf">
            <span className="label">LAST VRF OUTPUT</span>
            <code>{m ? `0x${hex(m.lastRandomness)}…` : "…"}</code>
            <span className="muted">every tick is one verifiable-randomness callback</span>
          </div>
        </section>

        <section className="card trade">
          <div className="label">YOUR DESK</div>
          <div className="wallet muted">
            burner {short(burner.publicKey)} · {sol === null ? "…" : `${fmt(sol, 3)} SOL`}
          </div>

          {!me?.entered ? (
            <div className="join">
              <p>Join the round with 10,000 chips. After one Solana transaction to join, every trade is free and confirms in milliseconds.</p>
              {sol !== null && sol < 0.01 ? (
                cluster.name === "devnet" ? (
                  <button className="btn" disabled={!!busy} onClick={fund}>
                    {busy === "Airdrop" ? "Requesting…" : "Get devnet SOL"}
                  </button>
                ) : null
              ) : null}
              <button className="btn primary" disabled={!!busy || !m?.open || (sol ?? 0) < 0.005} onClick={join}>
                {busy === "Join" ? "Joining…" : "Join round"}
              </button>
            </div>
          ) : (
            <>
              <div className="equity">
                <div>
                  <div className="label">EQUITY</div>
                  <div className="big">{fmt(myEq)}</div>
                </div>
                <div className={myPnl >= 0 ? "pnl up" : "pnl down"}>
                  {myPnl >= 0 ? "+" : ""}
                  {fmt(myPnl)}
                </div>
              </div>
              <div className="position">
                {me.size === 0 ? (
                  <span className="muted">Flat. Pick a side.</span>
                ) : (
                  <span>
                    <b className={me.size > 0 ? "up" : "down"}>{me.size > 0 ? "LONG" : "SHORT"}</b>{" "}
                    {fmt(Math.abs(me.size), 3)} @ {fmt(me.entryPrice, 3)}
                  </span>
                )}
              </div>
              <label className="size">
                <span className="label">SIZE (chips)</span>
                <input
                  type="range"
                  min={100}
                  max={Math.max(100, maxSize)}
                  step={100}
                  value={Math.min(size, Math.max(100, maxSize))}
                  onChange={(e) => setSize(Number(e.target.value))}
                />
                <span>
                  {fmt(Math.min(size, maxSize), 0)} · {me.cash > 0 ? fmt(Math.min(size, maxSize) / me.cash, 1) : "0"}x
                </span>
              </label>
              <div className="buttons">
                <button className="btn long" disabled={!inRound || !m?.open || !!busy} onClick={() => trade(1)}>
                  LONG
                </button>
                <button className="btn short" disabled={!inRound || !m?.open || !!busy} onClick={() => trade(-1)}>
                  SHORT
                </button>
                <button className="btn" disabled={!inRound || !m?.open || !!busy || me.size === 0} onClick={() => trade(0)}>
                  FLAT
                </button>
              </div>
              <div className="meta muted">
                {latency !== null ? `last trade confirmed on the rollup in ${latency} ms · fee 0` : "trades on the rollup cost 0 fees"}
              </div>
              <button className="btn ghost" disabled={!inRound || !!busy} onClick={cashOut}>
                {me.cashedOut ? "Cashed out" : busy === "Cash out" ? "Settling…" : "Cash out to Solana"}
              </button>
            </>
          )}
        </section>

        <section className="card board">
          <div className="label">LEADERBOARD</div>
          {ranked.length === 0 ? (
            <p className="muted">No traders yet. Be the first.</p>
          ) : (
            <ol>
              {ranked.map((t, i) => {
                const eq = equity(t, price);
                const mine = t.owner.equals(burner.publicKey);
                return (
                  <li key={t.address.toBase58()} className={mine ? "mine" : ""}>
                    <span className="rank">{i + 1}</span>
                    <span className="who">{mine ? "you" : short(t.owner)}</span>
                    <span className={t.size > 0 ? "side up" : t.size < 0 ? "side down" : "side muted"}>
                      {t.size > 0 ? "LONG" : t.size < 0 ? "SHORT" : t.cashedOut ? "OUT" : "FLAT"}
                    </span>
                    <span className={eq >= 10_000 ? "eq up" : "eq down"}>{fmt(eq, 0)}</span>
                  </li>
                );
              })}
            </ol>
          )}
        </section>
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

function Landing() {
  const [value, setValue] = useState("");
  return (
    <div className="landing">
      <img src="/favicon.svg" alt="" width={64} height={64} />
      <h1>Riptide</h1>
      <p>A real-time trading arena where the price is pure verifiable randomness, trades are free, and every round settles on Solana.</p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          const q = new URLSearchParams(window.location.search);
          q.set("market", value.trim());
          window.location.search = q.toString();
        }}
      >
        <input placeholder="Round market address" value={value} onChange={(e) => setValue(e.target.value)} />
        <button className="btn primary">Open round</button>
      </form>
      <p className="muted">Start one with <code>npm run round:new</code>.</p>
    </div>
  );
}
