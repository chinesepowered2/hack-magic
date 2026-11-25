# Riptide demo video: storyboard and narration

Target length: about 2 minutes. Record the screen at 1920x1080 with a round
running on devnet (`./scripts/deploy-devnet.sh`, then `npm run keeper`,
`npm run bots -- "" 4` and `npm run dev`). Each scene's narration is one audio
clip; the file names match the `## Scene` numbers.

| # | Screen | Length |
| - | ------ | ------ |
| 1 | Landing page, then the live chart ticking | 0:00 to 0:15 |
| 2 | Click Join; the wallet balance and chips appear | 0:15 to 0:30 |
| 3 | Go LONG, then SHORT, then FLAT; zoom on the latency and 0 fee readout | 0:30 to 0:55 |
| 4 | A riptide wave hits; the toast and the wave dot on the chart | 0:55 to 1:10 |
| 5 | Split screen: keeper log with tick numbers, and `lib.rs` at `consume_tide` | 1:10 to 1:30 |
| 6 | Leaderboard, Cash out, then the trader account on a devnet explorer | 1:30 to 1:50 |
| 7 | README architecture diagram, then the Riptide logo | 1:50 to 2:05 |

## Scene 1

This is Riptide. A trading arena on Solana, where the market moves on pure, verifiable randomness, and every single trade is free.

## Scene 2

Joining a round is one Solana transaction. You get ten thousand chips, and your account is delegated to a MagicBlock Ephemeral Rollup, where the whole round is played.

## Scene 3

From here, it's all real time. Go long. Flip short. Go flat. Every trade lands on chain in a fraction of a second, with zero fees and no wallet popups, so you can trade as fast as you can think.

## Scene 4

And then, a riptide wave. Without warning, the tide can surge or crash by up to eight percent in a single tick. Ride it, or get wiped out, and anyone can liquidate you.

## Scene 5

Here's the twist: nobody controls the price. Not us, not the round's creator. The only thing that can move it is a callback signed by MagicBlock's VRF, delivering fresh randomness inside the rollup, twice every second.

## Scene 6

When the round ends, you cash out. Your final balance is committed back to Solana and your account is undelegated, so what you won really is yours.

## Scene 7

Riptide. Ephemeral Rollups for speed, VRF for a fair market, and Solana for settlement. Come catch the tide.
