# Riptide — Solana Blitz v10 submission (draft answers)

Everything below is ready to paste into https://build.magicblock.app/?stage=blitz#submit
after signing in with Telegram. Field names match the form's four steps.

## 1 · Purpose

**Project name**

Riptide

**One-liner**

A real-time trading arena where the market is pure verifiable randomness, every trade is free, and every round settles on Solana.

**Description**

Riptide is an Entertainment Finance game built on MagicBlock. Each round opens a synthetic market whose price, "the tide", moves only when MagicBlock's VRF oracle delivers fresh randomness: one signed callback per tick, twice a second. Players join with 10,000 chips and go long or short with up to 5x leverage. Every trade executes inside a MagicBlock Ephemeral Rollup with zero fees and sub-second confirmation, so the game feels instant even though it is fully onchain. Rare "riptide waves" move the price 3-8% in a single tick, wiped-out positions can be liquidated by anyone, and when the round ends each trader's final balance is committed back to Solana and undelegated, so what you won really is yours.

Nobody can steer the chart: there is no admin set-price instruction, and the keeper that asks for randomness can never choose it. The price is a pure function of VRF output stored onchain, so any tick can be re-derived and audited.

**Categories (up to 3)**

Games, DeFi, Infra

**GitHub project repo**

https://github.com/chinesepowered2/hack-magic

**Pitch & demo**

demo/riptide-demo.mp4 in this repo (upload it to YouTube/Drive and paste the link here)

**Explorer link (integration proof)**

https://solana.fm/address/6ix99CDMjmnRcZut6NkRMCi1D9gV1Y1xXcVNsVD3hjf?cluster=devnet-solana (program). Demo round market: 7mXgYKXB8ic3MDfFxubW2eX9X6f7u263mzm4bz1c5mZP; the trader account settled back to Solana in the demo: https://solana.fm/address/HQ3wvYQ87rLHcDfMymTw2k4pxC7Y719s3dggfYCvwit3?cluster=devnet-solana

**On-chain addresses**

- Program: 6ix99CDMjmnRcZut6NkRMCi1D9gV1Y1xXcVNsVD3hjf (also in Anchor.toml and client/idl/riptide.json)
- Wallet: 2WPWwuhHdUeyZuB4PzpKaLkh4oRtLikXNVRY9oVfZXvE (deploy/keeper wallet for the demo round)

## 2 · About you

**Role(s)**

Developer (solo builder)

**Team**

Solo. (Add teammates here if any.)

## 3 · Proof of Work

**What was built for Blitz**

- Anchor program (`programs/riptide`) with the full round lifecycle: create/delegate on Solana, enter/trade/liquidate/request_tide/consume_tide on the Ephemeral Rollup, cash_out/settle_market back to Solana.
- Price moves only via the VRF-signed `consume_tide` callback (`#[vrf_callback]`), requested through MagicBlock's ephemeral oracle queue; tide math in `programs/riptide/src/tide.rs` is unit-tested.
- TypeScript client, permissionless keeper (requests a tide every 500 ms and liquidates wiped-out traders), demo trading bots, and a React web app with a live chart, trading desk and leaderboard over the rollup websocket.
- End-to-end tests on MagicSVM (`tests-magicsvm-rs`) that run Solana, the rollup and the VRF oracle in one process: VRF-driven ticks and round close, a full two-trader round with exact PnL and settlement equality between rollup and Solana copies, and permissionless liquidation of wiped-out positions.
- Deployed to Solana devnet with a live round on MagicBlock's devnet rollup; the demo video shows a real round: join (one Solana tx), zero-fee sub-second trades, a riptide wave, the keeper log beside `consume_tide`, and cash-out settling the trader account back on Solana.

**How to verify**

```bash
npm install
cargo build-sbf --manifest-path programs/riptide/Cargo.toml
npm test                 # unit tests + MagicSVM end-to-end tests
./scripts/deploy-devnet.sh   # deploy to devnet and open a round
npm run keeper
npm run dev              # open the printed URL with ?market=<address>
```

## 4 · Boost

- X/Twitter post: optional, add your handle.
- Anything else that helps: the README's sponsor-technology table maps every MagicBlock feature used (delegation, zero-fee execution, commit/undelegate, VRF, MagicSVM) to the exact code location.

## Demo video

`demo/riptide-demo.mp4` in this repo (also uploaded at DEMO_VIDEO_URL). ~2 minutes,
seven scenes following `demo/script.md`, narrated with a synthesized voiceover
(audio clips in `demo/audio/`, git-ignored).

## Submission checklist

1. Make the repo public (judges must be able to open it).
2. Sign in at build.magicblock.app with Telegram, open the Blitz v10 submission form.
3. Paste the answers above; upload or link the demo video.
4. Submit before the deadline (Nov 27, 9:00 AM EST / 6:00 AM PT).
