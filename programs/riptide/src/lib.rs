//! Riptide: a real-time, fully onchain trading arena on Solana.
//!
//! A round's price ("the tide") is moved only by verifiable randomness, one VRF
//! callback per tick, inside a MagicBlock Ephemeral Rollup. Players go long or
//! short with game chips at zero fees and sub-second latency, and when the round
//! ends every account is committed back to Solana with its final balance.
//!
//! Lifecycle
//!   Solana (base layer):   create_market -> delegate_market
//!                          join_market   -> delegate_trader
//!   Ephemeral Rollup:      enter, request_tide -> consume_tide (VRF), trade,
//!                          liquidate, close_round, commit_market
//!   Back to Solana:        cash_out (trader), settle_market (market)

use anchor_lang::prelude::*;
use ephemeral_rollups_sdk::{
    anchor::{commit, delegate, ephemeral, vrf, vrf_callback},
    cpi::DelegateConfig,
    ephem::MagicIntentBundleBuilder,
    vrf::{
        instructions::{create_request_scoped_randomness_ix, RequestRandomnessParams},
        types::SerializableAccountMeta,
    },
};

pub mod tide;

declare_id!("6ix99CDMjmnRcZut6NkRMCi1D9gV1Y1xXcVNsVD3hjf");

pub const MARKET_SEED: &[u8] = b"riptide-market";
pub const TRADER_SEED: &[u8] = b"riptide-trader";

/// Prices are fixed point with 6 decimals: 100_000_000 = 100.0 chips.
pub const PRICE_SCALE: u64 = 1_000_000;
/// Chip balances are fixed point with 6 decimals.
pub const CHIP: i64 = 1_000_000;
/// Every trader starts a round with 10,000 chips.
pub const STARTING_CHIPS: i64 = 10_000 * CHIP;
/// Opening price of every round.
pub const OPEN_PRICE: u64 = 100 * PRICE_SCALE;
/// Position notional may be at most this multiple of the trader's equity.
pub const MAX_LEVERAGE: i64 = 5;
/// Most traders one round can hold.
pub const MAX_TRADERS: usize = 32;
/// Number of recent prices kept onchain for the chart.
pub const HISTORY_LEN: usize = 64;

pub const STATUS_OPEN: u8 = 0;
pub const STATUS_CLOSED: u8 = 1;

#[ephemeral]
#[program]
pub mod riptide {
    use super::*;

    // ---------------------------------------------------------------------
    // Base layer
    // ---------------------------------------------------------------------

    /// Create a new round. Runs on Solana.
    pub fn create_market(
        ctx: Context<CreateMarket>,
        round_id: u64,
        max_ticks: u32,
        volatility_bps: u16,
    ) -> Result<()> {
        require!(max_ticks > 0, RiptideError::InvalidConfig);
        require!(
            (1..=tide::MAX_VOLATILITY_BPS).contains(&volatility_bps),
            RiptideError::InvalidConfig
        );
        let market = &mut ctx.accounts.market;
        market.authority = ctx.accounts.authority.key();
        market.round_id = round_id;
        market.price = OPEN_PRICE;
        market.open_price = OPEN_PRICE;
        market.high = OPEN_PRICE;
        market.low = OPEN_PRICE;
        market.tick = 0;
        market.max_ticks = max_ticks;
        market.volatility_bps = volatility_bps;
        market.status = STATUS_OPEN;
        market.last_wave = 0;
        market.waves = 0;
        market.roster_len = 0;
        market.roster = [Pubkey::default(); MAX_TRADERS];
        market.history = [OPEN_PRICE; HISTORY_LEN];
        market.last_randomness = [0; 32];
        market.bump = ctx.bumps.market;
        msg!("Riptide round {} created", round_id);
        Ok(())
    }

    /// Hand the market account to the Ephemeral Rollup. Runs on Solana.
    /// Pass the ER validator identity as the first remaining account to pin one.
    pub fn delegate_market(ctx: Context<DelegateMarket>, round_id: u64) -> Result<()> {
        let authority = ctx.accounts.authority.key();
        ctx.accounts.delegate_market(
            &ctx.accounts.authority,
            &[MARKET_SEED, authority.as_ref(), &round_id.to_le_bytes()],
            DelegateConfig {
                validator: ctx.remaining_accounts.first().map(|acc| acc.key()),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Create a trader account for a round, funded with starting chips. Runs on Solana.
    pub fn join_market(ctx: Context<JoinMarket>) -> Result<()> {
        let trader = &mut ctx.accounts.trader;
        trader.owner = ctx.accounts.owner.key();
        trader.market = ctx.accounts.market.key();
        trader.cash = STARTING_CHIPS;
        trader.size = 0;
        trader.entry_price = 0;
        trader.realized_pnl = 0;
        trader.trades = 0;
        trader.peak_equity = STARTING_CHIPS;
        trader.entered = false;
        trader.cashed_out = false;
        trader.bump = ctx.bumps.trader;
        Ok(())
    }

    /// Hand the trader account to the Ephemeral Rollup. Runs on Solana.
    pub fn delegate_trader(ctx: Context<DelegateTrader>) -> Result<()> {
        let market = ctx.accounts.market.key();
        let owner = ctx.accounts.owner.key();
        ctx.accounts.delegate_trader(
            &ctx.accounts.owner,
            &[TRADER_SEED, market.as_ref(), owner.as_ref()],
            DelegateConfig {
                validator: ctx.remaining_accounts.first().map(|acc| acc.key()),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Ephemeral Rollup (zero fees, real time)
    // ---------------------------------------------------------------------

    /// Put a delegated trader on the round's roster so they show on the leaderboard.
    pub fn enter(ctx: Context<Enter>) -> Result<()> {
        let market = &mut ctx.accounts.market;
        let trader = &mut ctx.accounts.trader;
        require!(market.status == STATUS_OPEN, RiptideError::RoundClosed);
        if trader.entered {
            return Ok(());
        }
        let len = market.roster_len as usize;
        require!(len < MAX_TRADERS, RiptideError::RoundFull);
        market.roster[len] = trader.key();
        market.roster_len += 1;
        trader.entered = true;
        msg!("Trader {} entered round {}", trader.owner, market.round_id);
        Ok(())
    }

    /// Ask the VRF oracle for fresh randomness to move the tide by one tick.
    /// Permissionless: the keeper calls it on a timer, and anyone else may too.
    pub fn request_tide(ctx: Context<RequestTide>, client_seed: u8) -> Result<()> {
        require!(
            ctx.accounts.market.status == STATUS_OPEN,
            RiptideError::RoundClosed
        );
        let ix = create_request_scoped_randomness_ix(RequestRandomnessParams {
            payer: ctx.accounts.payer.key(),
            oracle_queue: ctx.accounts.oracle_queue.key(),
            callback_program_id: ID,
            callback_discriminator: instruction::ConsumeTide::DISCRIMINATOR.to_vec(),
            caller_seed: [client_seed; 32],
            accounts_metas: Some(vec![SerializableAccountMeta {
                pubkey: ctx.accounts.market.key(),
                is_signer: false,
                is_writable: true,
            }]),
            callback_args: Some(vec![client_seed]),
            ..Default::default()
        });
        ctx.accounts
            .invoke_signed_vrf(&ctx.accounts.payer.to_account_info(), &ix)?;
        Ok(())
    }

    /// VRF callback: the only way the price ever moves.
    pub fn consume_tide(
        ctx: Context<ConsumeTide>,
        randomness: [u8; 32],
        _client_seed: u8,
    ) -> Result<()> {
        let market = &mut ctx.accounts.market;
        if market.status != STATUS_OPEN {
            msg!("Round closed, ignoring late randomness");
            return Ok(());
        }
        let step = tide::next_price(market.price, &randomness, market.volatility_bps);
        market.price = step.price;
        market.high = market.high.max(step.price);
        market.low = market.low.min(step.price);
        market.last_wave = step.wave;
        if step.wave != 0 {
            market.waves = market.waves.saturating_add(1);
        }
        market.last_randomness = randomness;
        market.tick = market.tick.saturating_add(1);
        let slot = (market.tick as usize) % HISTORY_LEN;
        market.history[slot] = step.price;
        if market.tick >= market.max_ticks {
            market.status = STATUS_CLOSED;
            msg!("Round {} closed at tick {}", market.round_id, market.tick);
        }
        msg!("tick={} price={} wave={}", market.tick, market.price, step.wave);
        Ok(())
    }

    /// Set the trader's position. `direction` is 1 for long, -1 for short and 0
    /// to go flat; `notional` is the chip value to open at the current price.
    /// Any open position is closed (and its PnL realized) first.
    pub fn trade(ctx: Context<Trade>, direction: i8, notional: u64) -> Result<()> {
        let market = &ctx.accounts.market;
        let trader = &mut ctx.accounts.trader;
        require!(market.status == STATUS_OPEN, RiptideError::RoundClosed);
        require!(trader.entered, RiptideError::NotEntered);
        require!(!trader.cashed_out, RiptideError::CashedOut);
        require!((-1..=1).contains(&direction), RiptideError::InvalidTrade);

        trader.close_position(market.price)?;

        if direction != 0 {
            require!(notional > 0, RiptideError::InvalidTrade);
            let notional = i64::try_from(notional).map_err(|_| RiptideError::InvalidTrade)?;
            let max = trader
                .cash
                .checked_mul(MAX_LEVERAGE)
                .ok_or(RiptideError::MathOverflow)?;
            require!(notional <= max, RiptideError::OverLeveraged);
            let qty = tide::qty_for_notional(notional, market.price)?;
            require!(qty > 0, RiptideError::InvalidTrade);
            trader.size = qty * direction as i64;
            trader.entry_price = market.price;
        }
        trader.trades = trader.trades.saturating_add(1);
        let equity = trader.equity(market.price)?;
        trader.peak_equity = trader.peak_equity.max(equity);
        msg!(
            "trade owner={} dir={} size={} price={} cash={}",
            trader.owner,
            direction,
            trader.size,
            market.price,
            trader.cash
        );
        Ok(())
    }

    /// Permissionless: close a position whose equity has hit zero.
    pub fn liquidate(ctx: Context<Liquidate>) -> Result<()> {
        let price = ctx.accounts.market.price;
        let trader = &mut ctx.accounts.trader;
        require!(trader.size != 0, RiptideError::NotLiquidatable);
        require!(trader.equity(price)? <= 0, RiptideError::NotLiquidatable);
        trader.close_position(price)?;
        msg!("Liquidated {}", trader.owner);
        Ok(())
    }

    /// Authority may end a round early.
    pub fn close_round(ctx: Context<CloseRound>) -> Result<()> {
        ctx.accounts.market.status = STATUS_CLOSED;
        Ok(())
    }

    /// Snapshot the live market state to Solana without leaving the rollup.
    pub fn commit_market(ctx: Context<CommitMarket>) -> Result<()> {
        ctx.accounts.market.exit(&crate::ID)?;
        MagicIntentBundleBuilder::new(
            ctx.accounts.payer.to_account_info(),
            ctx.accounts.magic_context.to_account_info(),
            ctx.accounts.magic_program.to_account_info(),
        )
        .commit(&[ctx.accounts.market.to_account_info()])
        .build_and_invoke()?;
        Ok(())
    }

    /// Close any open position at the live price and settle the trader's final
    /// chips back on Solana (commit + undelegate).
    pub fn cash_out(ctx: Context<CashOut>) -> Result<()> {
        let price = ctx.accounts.market.price;
        {
            let trader = &mut ctx.accounts.trader;
            require!(!trader.cashed_out, RiptideError::CashedOut);
            trader.close_position(price)?;
            trader.cashed_out = true;
            msg!("Cash out {} with {} chips", trader.owner, trader.cash);
        }
        ctx.accounts.trader.exit(&crate::ID)?;
        MagicIntentBundleBuilder::new(
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.magic_context.to_account_info(),
            ctx.accounts.magic_program.to_account_info(),
        )
        .commit_and_undelegate(&[ctx.accounts.trader.to_account_info()])
        .build_and_invoke()?;
        Ok(())
    }

    /// Settle a closed round's market on Solana (commit + undelegate).
    pub fn settle_market(ctx: Context<SettleMarket>) -> Result<()> {
        require!(
            ctx.accounts.market.status == STATUS_CLOSED,
            RiptideError::RoundStillOpen
        );
        ctx.accounts.market.exit(&crate::ID)?;
        MagicIntentBundleBuilder::new(
            ctx.accounts.authority.to_account_info(),
            ctx.accounts.magic_context.to_account_info(),
            ctx.accounts.magic_program.to_account_info(),
        )
        .commit_and_undelegate(&[ctx.accounts.market.to_account_info()])
        .build_and_invoke()?;
        Ok(())
    }
}

// -------------------------------------------------------------------------
// State
// -------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct Market {
    pub authority: Pubkey,
    pub round_id: u64,
    pub price: u64,
    pub open_price: u64,
    pub high: u64,
    pub low: u64,
    pub tick: u32,
    pub max_ticks: u32,
    pub volatility_bps: u16,
    pub status: u8,
    /// 1 = up-wave, -1 = down-wave, 0 = calm, for the last tick.
    pub last_wave: i8,
    pub waves: u16,
    pub roster_len: u8,
    pub roster: [Pubkey; MAX_TRADERS],
    pub history: [u64; HISTORY_LEN],
    pub last_randomness: [u8; 32],
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Trader {
    pub owner: Pubkey,
    pub market: Pubkey,
    /// Chips not tied up in open PnL (6 decimals).
    pub cash: i64,
    /// Signed position quantity (6 decimals): positive long, negative short.
    pub size: i64,
    pub entry_price: u64,
    pub realized_pnl: i64,
    pub trades: u32,
    pub peak_equity: i64,
    pub entered: bool,
    pub cashed_out: bool,
    pub bump: u8,
}

impl Trader {
    pub fn unrealized(&self, price: u64) -> Result<i64> {
        tide::pnl(self.size, self.entry_price, price)
    }

    pub fn equity(&self, price: u64) -> Result<i64> {
        self.cash
            .checked_add(self.unrealized(price)?)
            .ok_or(error!(RiptideError::MathOverflow))
    }

    /// Realize PnL at `price`. Equity cannot go below zero (a wipe-out).
    pub fn close_position(&mut self, price: u64) -> Result<()> {
        if self.size == 0 {
            return Ok(());
        }
        let pnl = self.unrealized(price)?;
        let new_cash = self
            .cash
            .checked_add(pnl)
            .ok_or(RiptideError::MathOverflow)?
            .max(0);
        self.realized_pnl = self
            .realized_pnl
            .checked_add(new_cash - self.cash)
            .ok_or(RiptideError::MathOverflow)?;
        self.cash = new_cash;
        self.size = 0;
        self.entry_price = 0;
        Ok(())
    }
}

// -------------------------------------------------------------------------
// Accounts
// -------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(round_id: u64)]
pub struct CreateMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + Market::INIT_SPACE,
        seeds = [MARKET_SEED, authority.key().as_ref(), &round_id.to_le_bytes()],
        bump
    )]
    pub market: Box<Account<'info, Market>>,
    pub system_program: Program<'info, System>,
}

#[delegate]
#[derive(Accounts)]
#[instruction(round_id: u64)]
pub struct DelegateMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: the market PDA being delegated
    #[account(
        mut,
        del,
        seeds = [MARKET_SEED, authority.key().as_ref(), &round_id.to_le_bytes()],
        bump
    )]
    pub market: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct JoinMarket<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: only its address is used; it may already be delegated to the rollup
    pub market: UncheckedAccount<'info>,
    #[account(
        init,
        payer = owner,
        space = 8 + Trader::INIT_SPACE,
        seeds = [TRADER_SEED, market.key().as_ref(), owner.key().as_ref()],
        bump
    )]
    pub trader: Account<'info, Trader>,
    pub system_program: Program<'info, System>,
}

#[delegate]
#[derive(Accounts)]
pub struct DelegateTrader<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: only its address is used, for the trader seeds
    pub market: UncheckedAccount<'info>,
    /// CHECK: the trader PDA being delegated
    #[account(
        mut,
        del,
        seeds = [TRADER_SEED, market.key().as_ref(), owner.key().as_ref()],
        bump
    )]
    pub trader: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Enter<'info> {
    pub owner: Signer<'info>,
    #[account(mut)]
    pub market: Box<Account<'info, Market>>,
    #[account(
        mut,
        has_one = owner,
        has_one = market,
        seeds = [TRADER_SEED, market.key().as_ref(), owner.key().as_ref()],
        bump = trader.bump
    )]
    pub trader: Account<'info, Trader>,
}

#[vrf]
#[derive(Accounts)]
pub struct RequestTide<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        seeds = [MARKET_SEED, market.authority.as_ref(), &market.round_id.to_le_bytes()],
        bump = market.bump
    )]
    pub market: Box<Account<'info, Market>>,
    /// CHECK: validated by the VRF program when it processes the request
    #[account(mut)]
    pub oracle_queue: UncheckedAccount<'info>,
}

#[vrf_callback]
#[derive(Accounts)]
pub struct ConsumeTide<'info> {
    #[account(mut)]
    pub market: Box<Account<'info, Market>>,
}

#[derive(Accounts)]
pub struct Trade<'info> {
    pub owner: Signer<'info>,
    pub market: Box<Account<'info, Market>>,
    #[account(
        mut,
        has_one = owner,
        has_one = market,
        seeds = [TRADER_SEED, market.key().as_ref(), owner.key().as_ref()],
        bump = trader.bump
    )]
    pub trader: Account<'info, Trader>,
}

#[derive(Accounts)]
pub struct Liquidate<'info> {
    pub market: Box<Account<'info, Market>>,
    #[account(mut, has_one = market)]
    pub trader: Account<'info, Trader>,
}

#[derive(Accounts)]
pub struct CloseRound<'info> {
    pub authority: Signer<'info>,
    #[account(mut, has_one = authority)]
    pub market: Box<Account<'info, Market>>,
}

#[commit]
#[derive(Accounts)]
pub struct CommitMarket<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut)]
    pub market: Box<Account<'info, Market>>,
}

#[commit]
#[derive(Accounts)]
pub struct CashOut<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    pub market: Box<Account<'info, Market>>,
    #[account(
        mut,
        has_one = owner,
        has_one = market,
        seeds = [TRADER_SEED, market.key().as_ref(), owner.key().as_ref()],
        bump = trader.bump
    )]
    pub trader: Account<'info, Trader>,
}

#[commit]
#[derive(Accounts)]
pub struct SettleMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut, has_one = authority)]
    pub market: Box<Account<'info, Market>>,
}

#[error_code]
pub enum RiptideError {
    #[msg("Invalid market configuration")]
    InvalidConfig,
    #[msg("This round is closed")]
    RoundClosed,
    #[msg("This round is still open")]
    RoundStillOpen,
    #[msg("This round is full")]
    RoundFull,
    #[msg("Enter the round before trading")]
    NotEntered,
    #[msg("This trader has already cashed out")]
    CashedOut,
    #[msg("Invalid trade")]
    InvalidTrade,
    #[msg("Position exceeds the leverage limit")]
    OverLeveraged,
    #[msg("Position is not liquidatable")]
    NotLiquidatable,
    #[msg("Math overflow")]
    MathOverflow,
}
