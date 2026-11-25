// Riptide TypeScript client: PDAs, transaction builders and state decoding.
// Shared by the web app, the keeper and the CLI.

import * as anchorNs from "@coral-xyz/anchor";
import type { Idl, Program as ProgramT } from "@coral-xyz/anchor";
// Node loads @coral-xyz/anchor as CommonJS (exports under `default`), while
// Vite loads its ESM browser build (named exports). Accept both.
const anchor: typeof anchorNs = (anchorNs as any).default ?? anchorNs;
const { AnchorProvider, BN, Program } = anchor;
type BN = InstanceType<typeof BN>;
type Program = ProgramT;
import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  type Commitment,
  type TransactionInstruction,
} from "@solana/web3.js";
import idlJson from "./idl/riptide.json" with { type: "json" };

export const IDL = idlJson as Idl;
export const PROGRAM_ID = new PublicKey(idlJson.address);

export const MARKET_SEED = Buffer.from("riptide-market");
export const TRADER_SEED = Buffer.from("riptide-trader");

export const PRICE_SCALE = 1_000_000;
export const CHIP = 1_000_000;
export const STARTING_CHIPS = 10_000;
export const MAX_LEVERAGE = 5;
export const HISTORY_LEN = 64;

/** MagicBlock VRF queue that serves requests made inside an Ephemeral Rollup. */
export const EPHEMERAL_ORACLE_QUEUE = new PublicKey(
  "5hBR571xnXppuCPveTrctfTU7tJLSN94nq7kv7FRK5Tc",
);
/** Local MagicBlock stack (mb-test-validator + ephemeral-validator) VRF queue. */
export const LOCAL_EPHEMERAL_ORACLE_QUEUE = new PublicKey(
  "Sc9MJUngNbQXSXGP3F67KvKwVnhaYn6kcioxXNVowYT",
);

export interface Cluster {
  name: string;
  /** Solana base layer RPC. */
  baseRpc: string;
  /** MagicBlock Ephemeral Rollup RPC. */
  erRpc: string;
  /** Identity of the ER validator that accounts are delegated to. */
  erValidator: PublicKey;
  oracleQueue: PublicKey;
}

export const CLUSTERS: Record<string, Cluster> = {
  devnet: {
    name: "devnet",
    baseRpc: "https://api.devnet.solana.com",
    erRpc: "https://devnet-as.magicblock.app",
    erValidator: new PublicKey("MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57"),
    oracleQueue: EPHEMERAL_ORACLE_QUEUE,
  },
  localnet: {
    name: "localnet",
    baseRpc: "http://127.0.0.1:8899",
    erRpc: "http://127.0.0.1:7799",
    erValidator: new PublicKey("mAGicPQYBMvcYveUZA5F5UNNwyHvfYh5xkLS2Fr1mev"),
    oracleQueue: LOCAL_EPHEMERAL_ORACLE_QUEUE,
  },
};

export function wsFor(rpc: string): string {
  return rpc.replace(/^https:/, "wss:").replace(/^http:\/\/127\.0\.0\.1:(\d+)/, (_m, port) => {
    return `ws://127.0.0.1:${Number(port) + 1}`;
  });
}

// ---------------------------------------------------------------------------
// PDAs
// ---------------------------------------------------------------------------

export function roundIdBytes(roundId: BN | number | bigint): Buffer {
  const bn = BN.isBN(roundId) ? roundId : new BN(roundId.toString());
  return bn.toArrayLike(Buffer, "le", 8);
}

export function marketPda(authority: PublicKey, roundId: BN | number | bigint): PublicKey {
  return PublicKey.findProgramAddressSync(
    [MARKET_SEED, authority.toBuffer(), roundIdBytes(roundId)],
    PROGRAM_ID,
  )[0];
}

export function traderPda(market: PublicKey, owner: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [TRADER_SEED, market.toBuffer(), owner.toBuffer()],
    PROGRAM_ID,
  )[0];
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

export interface MarketState {
  address: PublicKey;
  authority: PublicKey;
  roundId: number;
  price: number;
  openPrice: number;
  high: number;
  low: number;
  tick: number;
  maxTicks: number;
  volatilityBps: number;
  open: boolean;
  lastWave: number;
  waves: number;
  roster: PublicKey[];
  /** Up to HISTORY_LEN most recent prices, oldest first. */
  history: number[];
  lastRandomness: Uint8Array;
}

export interface TraderState {
  address: PublicKey;
  owner: PublicKey;
  market: PublicKey;
  /** Chips, as a decimal number. */
  cash: number;
  /** Signed position quantity (units of the asset). */
  size: number;
  entryPrice: number;
  realizedPnl: number;
  trades: number;
  peakEquity: number;
  entered: boolean;
  cashedOut: boolean;
}

const num = (v: BN) => Number(v.toString());

export function decodeMarket(program: Program, address: PublicKey, data: Buffer): MarketState {
  const m: any = program.coder.accounts.decode("Market", data);
  const tick: number = m.tick;
  const raw: number[] = m.history.map((h: BN) => num(h) / PRICE_SCALE);
  // history[tick % 64] holds the latest price; unroll the ring oldest-first.
  const count = Math.min(tick + 1, HISTORY_LEN);
  const history: number[] = [];
  for (let i = count - 1; i >= 0; i--) {
    history.push(raw[(((tick - i) % HISTORY_LEN) + HISTORY_LEN) % HISTORY_LEN]);
  }
  return {
    address,
    authority: m.authority,
    roundId: num(m.roundId),
    price: num(m.price) / PRICE_SCALE,
    openPrice: num(m.openPrice) / PRICE_SCALE,
    high: num(m.high) / PRICE_SCALE,
    low: num(m.low) / PRICE_SCALE,
    tick,
    maxTicks: m.maxTicks,
    volatilityBps: m.volatilityBps,
    open: m.status === 0,
    lastWave: m.lastWave,
    waves: m.waves,
    roster: (m.roster as PublicKey[]).slice(0, m.rosterLen),
    history,
    lastRandomness: Uint8Array.from(m.lastRandomness),
  };
}

export function decodeTrader(program: Program, address: PublicKey, data: Buffer): TraderState {
  const t: any = program.coder.accounts.decode("Trader", data);
  return {
    address,
    owner: t.owner,
    market: t.market,
    cash: num(t.cash) / CHIP,
    size: num(t.size) / 1_000_000,
    entryPrice: num(t.entryPrice) / PRICE_SCALE,
    realizedPnl: num(t.realizedPnl) / CHIP,
    trades: t.trades,
    peakEquity: num(t.peakEquity) / CHIP,
    entered: t.entered,
    cashedOut: t.cashedOut,
  };
}

export function unrealized(t: TraderState, price: number): number {
  return t.size === 0 ? 0 : t.size * (price - t.entryPrice);
}

export function equity(t: TraderState, price: number): number {
  return t.cash + unrealized(t, price);
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/** Minimal wallet for AnchorProvider: we always sign with a local keypair. */
function keypairWallet(kp: Keypair) {
  return {
    publicKey: kp.publicKey,
    payer: kp,
    async signTransaction<T>(tx: T): Promise<T> {
      (tx as Transaction).partialSign(kp);
      return tx;
    },
    async signAllTransactions<T>(txs: T[]): Promise<T[]> {
      txs.forEach((tx) => (tx as Transaction).partialSign(kp));
      return txs;
    },
  };
}

export class RiptideClient {
  readonly base: Connection;
  readonly er: Connection;
  readonly program: Program;
  readonly erProgram: Program;

  constructor(
    readonly cluster: Cluster,
    readonly signer: Keypair,
    commitment: Commitment = "confirmed",
  ) {
    this.base = new Connection(cluster.baseRpc, { commitment, wsEndpoint: wsFor(cluster.baseRpc) });
    this.er = new Connection(cluster.erRpc, { commitment, wsEndpoint: wsFor(cluster.erRpc) });
    const wallet = keypairWallet(signer) as any;
    this.program = new Program(IDL, new AnchorProvider(this.base, wallet, { commitment }));
    this.erProgram = new Program(IDL, new AnchorProvider(this.er, wallet, { commitment }));
  }

  get me(): PublicKey {
    return this.signer.publicKey;
  }

  /** Sign and send instructions to the base layer or the rollup. */
  async send(
    layer: "base" | "er",
    ixs: TransactionInstruction[],
    extraSigners: Keypair[] = [],
  ): Promise<string> {
    const conn = layer === "base" ? this.base : this.er;
    const tx = new Transaction().add(...ixs);
    const { blockhash, lastValidBlockHeight } = await conn.getLatestBlockhash();
    tx.recentBlockhash = blockhash;
    tx.feePayer = this.me;
    tx.sign(this.signer, ...extraSigners);
    const sig = await conn.sendRawTransaction(tx.serialize(), {
      // The rollup has no fee market and its preflight cannot see accounts that
      // are still being cloned from the base layer, so skip it there.
      skipPreflight: layer === "er",
    });
    const res = await conn.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight });
    if (res.value.err) {
      throw new Error(`${layer} tx ${sig} failed: ${JSON.stringify(res.value.err)}`);
    }
    return sig;
  }

  // ----- base layer -----

  async createMarket(roundId: number, maxTicks: number, volatilityBps: number): Promise<PublicKey> {
    const market = marketPda(this.me, roundId);
    const create = await this.program.methods
      .createMarket(new BN(roundId), maxTicks, volatilityBps)
      .accountsPartial({ authority: this.me, market })
      .instruction();
    const delegate = await this.program.methods
      .delegateMarket(new BN(roundId))
      .accountsPartial({ authority: this.me, market })
      .remainingAccounts([{ pubkey: this.cluster.erValidator, isSigner: false, isWritable: false }])
      .instruction();
    await this.send("base", [create, delegate]);
    return market;
  }

  /** Join a round on Solana and move the trader account into the rollup. */
  async join(market: PublicKey): Promise<PublicKey> {
    const trader = traderPda(market, this.me);
    const existing = await this.base.getAccountInfo(trader);
    const ixs: TransactionInstruction[] = [];
    if (!existing) {
      ixs.push(
        await this.program.methods
          .joinMarket()
          .accountsPartial({ owner: this.me, market, trader })
          .instruction(),
      );
    }
    if (!existing || existing.owner.equals(PROGRAM_ID)) {
      ixs.push(
        await this.program.methods
          .delegateTrader()
          .accountsPartial({ owner: this.me, market, trader })
          .remainingAccounts([
            { pubkey: this.cluster.erValidator, isSigner: false, isWritable: false },
          ])
          .instruction(),
      );
    }
    if (ixs.length) await this.send("base", ixs);
    return trader;
  }

  // ----- ephemeral rollup -----

  async enter(market: PublicKey): Promise<string> {
    const ix = await this.erProgram.methods
      .enter()
      .accountsPartial({ owner: this.me, market, trader: traderPda(market, this.me) })
      .instruction();
    return this.send("er", [ix]);
  }

  async requestTide(market: PublicKey, clientSeed: number): Promise<string> {
    const ix = await this.erProgram.methods
      .requestTide(clientSeed & 0xff)
      .accountsPartial({ payer: this.me, market, oracleQueue: this.cluster.oracleQueue })
      .instruction();
    return this.send("er", [ix]);
  }

  /** direction: 1 long, -1 short, 0 flat. notional is in chips. */
  async trade(market: PublicKey, direction: -1 | 0 | 1, notionalChips: number): Promise<string> {
    const notional = new BN(Math.floor(notionalChips * CHIP).toString());
    const ix = await this.erProgram.methods
      .trade(direction, notional)
      .accountsPartial({ owner: this.me, market, trader: traderPda(market, this.me) })
      .instruction();
    return this.send("er", [ix]);
  }

  async liquidate(market: PublicKey, trader: PublicKey): Promise<string> {
    const ix = await this.erProgram.methods
      .liquidate()
      .accountsPartial({ market, trader })
      .instruction();
    return this.send("er", [ix]);
  }

  async closeRound(market: PublicKey): Promise<string> {
    const ix = await this.erProgram.methods
      .closeRound()
      .accountsPartial({ authority: this.me, market })
      .instruction();
    return this.send("er", [ix]);
  }

  async commitMarket(market: PublicKey): Promise<string> {
    const ix = await this.erProgram.methods
      .commitMarket()
      .accountsPartial({ payer: this.me, market })
      .instruction();
    return this.send("er", [ix]);
  }

  async cashOut(market: PublicKey): Promise<string> {
    const ix = await this.erProgram.methods
      .cashOut()
      .accountsPartial({ owner: this.me, market, trader: traderPda(market, this.me) })
      .instruction();
    return this.send("er", [ix]);
  }

  async settleMarket(market: PublicKey): Promise<string> {
    const ix = await this.erProgram.methods
      .settleMarket()
      .accountsPartial({ authority: this.me, market })
      .instruction();
    return this.send("er", [ix]);
  }

  // ----- reads -----

  async fetchMarket(market: PublicKey, layer: "base" | "er" = "er"): Promise<MarketState | null> {
    const info = await (layer === "er" ? this.er : this.base).getAccountInfo(market);
    return info ? decodeMarket(this.program, market, info.data) : null;
  }

  async fetchTraders(addresses: PublicKey[], layer: "base" | "er" = "er"): Promise<TraderState[]> {
    if (!addresses.length) return [];
    const infos = await (layer === "er" ? this.er : this.base).getMultipleAccountsInfo(addresses);
    return infos.flatMap((info, i) =>
      info && info.owner.equals(PROGRAM_ID)
        ? [decodeTrader(this.program, addresses[i], info.data)]
        : [],
    );
  }

  /** Live market updates from the rollup's websocket. */
  onMarket(market: PublicKey, cb: (m: MarketState) => void): () => void {
    const id = this.er.onAccountChange(
      market,
      (info) => cb(decodeMarket(this.program, market, info.data)),
      { commitment: "processed" },
    );
    return () => void this.er.removeAccountChangeListener(id);
  }
}
