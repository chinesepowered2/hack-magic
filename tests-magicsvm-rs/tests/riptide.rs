//! End-to-end tests of a full Riptide round on MagicSVM, which runs a Solana
//! base layer, an Ephemeral Rollup and the VRF oracle in one process.

use {
    anchor_lang::{prelude::Pubkey, system_program, AccountDeserialize, Id},
    magicsvm::{MagicSVM, TransactionTarget},
    riptide::{
        Market, Trader, CHIP, MARKET_SEED, MAX_LEVERAGE, OPEN_PRICE, STARTING_CHIPS,
        STATUS_CLOSED, STATUS_OPEN, TRADER_SEED,
    },
    solana_clock::Clock,
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_native_token::LAMPORTS_PER_SOL,
    solana_signer::Signer,
    test_utils::{
        airdrop, as_address, as_pubkey, boot, client_ix,
        delegate_buffer_pda_from_delegated_account_and_owner_program,
        delegation_metadata_pda_from_delegated_account,
        delegation_record_pda_from_delegated_account, is_delegated, program_id_from_idl, send_ix,
        sign_tx, DelegationProgram, MagicProgram, MAGIC_CONTEXT_ID,
    },
};

const VRF_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("Vrf1RNUjXmQGjmQrQLvJHs9SNkvDJEsRVFPkfSQUwGz");
const DEFAULT_EPHEMERAL_QUEUE: Pubkey =
    Pubkey::from_str_const("5hBR571xnXppuCPveTrctfTU7tJLSN94nq7kv7FRK5Tc");
const SLOT_HASHES: Pubkey = Pubkey::from_str_const("SysvarS1otHashes111111111111111111111111111");

const ROUND: u64 = 7;

struct Round {
    svm: MagicSVM,
    program: Pubkey,
    authority: Keypair,
    market: Pubkey,
}

fn ix(
    program: Pubkey,
    data: impl anchor_lang::InstructionData,
    accounts: impl anchor_lang::ToAccountMetas,
) -> Instruction {
    client_ix(as_address(program), data, accounts)
}

fn market_state(svm: &MagicSVM, target: TransactionTarget, market: Pubkey) -> Market {
    let data = svm
        .get_account_for(target, &as_address(market))
        .expect("market account")
        .data;
    Market::try_deserialize(&mut data.as_slice()).expect("market data")
}

fn trader_state(svm: &MagicSVM, target: TransactionTarget, trader: Pubkey) -> Trader {
    let data = svm
        .get_account_for(target, &as_address(trader))
        .expect("trader account")
        .data;
    Trader::try_deserialize(&mut data.as_slice()).expect("trader data")
}

/// Advance one slot so pending VRF requests are fulfilled; assert all succeed.
fn fulfill(svm: &mut MagicSVM, expected: usize) {
    let slot = svm.get_sysvar::<Clock>().slot;
    let results = svm.warp_to_slot(slot + 1);
    assert_eq!(results.len(), expected);
    for result in results {
        if let Err(failed) = result {
            panic!("callback failed: {:?}\nlogs: {:#?}", failed.err, failed.meta.logs);
        }
    }
}

/// Send an instruction that is expected to fail, returning its logs.
fn send_fails(
    svm: &mut MagicSVM,
    target: TransactionTarget,
    signers: &[&Keypair],
    instruction: Instruction,
) -> Vec<String> {
    let tx = sign_tx(signers, &[instruction], svm.latest_blockhash_for(target));
    let logs = match svm.send_transaction_to(target, tx) {
        Ok(_) => panic!("transaction unexpectedly succeeded"),
        Err(failed) => failed.meta.logs,
    };
    svm.expire_blockhash_for(target);
    logs
}

fn open_round(max_ticks: u32) -> Round {
    open_round_with(max_ticks, 40)
}

fn open_round_with(max_ticks: u32, volatility_bps: u16) -> Round {
    let authority = Keypair::new();
    let program = as_pubkey(&program_id_from_idl(env!("CARGO_MANIFEST_DIR"), "riptide.json"));
    let mut svm = boot(
        env!("CARGO_MANIFEST_DIR"),
        "riptide",
        as_address(program),
        &authority,
        10 * LAMPORTS_PER_SOL,
    );
    let auth = as_pubkey(&authority.pubkey());
    let (market, _) = Pubkey::find_program_address(
        &[MARKET_SEED, auth.as_ref(), &ROUND.to_le_bytes()],
        &program,
    );

    send_ix(
        &mut svm,
        TransactionTarget::Base,
        &[&authority],
        ix(
            program,
            riptide::instruction::CreateMarket {
                round_id: ROUND,
                max_ticks,
                volatility_bps,
            },
            riptide::accounts::CreateMarket {
                authority: auth,
                market,
                system_program: system_program::ID,
            },
        ),
        "create_market",
    );

    let mut delegate = ix(
        program,
        riptide::instruction::DelegateMarket { round_id: ROUND },
        riptide::accounts::DelegateMarket {
            authority: auth,
            buffer_market: delegate_buffer_pda_from_delegated_account_and_owner_program(
                &market, &program,
            ),
            delegation_record_market: delegation_record_pda_from_delegated_account(&market),
            delegation_metadata_market: delegation_metadata_pda_from_delegated_account(&market),
            market,
            owner_program: program,
            delegation_program: DelegationProgram::id(),
            system_program: system_program::ID,
        },
    );
    delegate
        .accounts
        .push(AccountMeta::new_readonly(svm.validator_identity(), false));
    send_ix(&mut svm, TransactionTarget::Base, &[&authority], delegate, "delegate_market");
    assert!(is_delegated(&svm, &as_address(market)));

    Round { svm, program, authority, market }
}

/// Join on Solana, delegate to the rollup and enter the round. Returns the trader PDA.
fn add_trader(round: &mut Round, owner: &Keypair) -> Pubkey {
    let Round { svm, program, market, .. } = round;
    let (program, market) = (*program, *market);
    airdrop(svm, &owner.pubkey(), 2 * LAMPORTS_PER_SOL);
    let me = as_pubkey(&owner.pubkey());
    let (trader, _) =
        Pubkey::find_program_address(&[TRADER_SEED, market.as_ref(), me.as_ref()], &program);

    send_ix(
        svm,
        TransactionTarget::Base,
        &[owner],
        ix(
            program,
            riptide::instruction::JoinMarket {},
            riptide::accounts::JoinMarket {
                owner: me,
                market,
                trader,
                system_program: system_program::ID,
            },
        ),
        "join_market",
    );
    let mut delegate = ix(
        program,
        riptide::instruction::DelegateTrader {},
        riptide::accounts::DelegateTrader {
            owner: me,
            market,
            buffer_trader: delegate_buffer_pda_from_delegated_account_and_owner_program(
                &trader, &program,
            ),
            delegation_record_trader: delegation_record_pda_from_delegated_account(&trader),
            delegation_metadata_trader: delegation_metadata_pda_from_delegated_account(&trader),
            trader,
            owner_program: program,
            delegation_program: DelegationProgram::id(),
            system_program: system_program::ID,
        },
    );
    delegate
        .accounts
        .push(AccountMeta::new_readonly(svm.validator_identity(), false));
    send_ix(svm, TransactionTarget::Base, &[owner], delegate, "delegate_trader");
    assert!(is_delegated(svm, &as_address(trader)));

    send_ix(
        svm,
        TransactionTarget::Ephemeral,
        &[owner],
        ix(
            program,
            riptide::instruction::Enter {},
            riptide::accounts::Enter { owner: me, market, trader },
        ),
        "enter",
    );
    trader
}

fn trade_ix(round: &Round, owner: &Keypair, trader: Pubkey, direction: i8, notional: u64) -> Instruction {
    ix(
        round.program,
        riptide::instruction::Trade { direction, notional },
        riptide::accounts::Trade {
            owner: as_pubkey(&owner.pubkey()),
            market: round.market,
            trader,
        },
    )
}

/// Request randomness on the rollup and let the oracle answer it.
fn tide(round: &mut Round, payer: &Keypair, seed: u8) {
    let (program_identity, _) = Pubkey::find_program_address(&[b"identity"], &round.program);
    let request = ix(
        round.program,
        riptide::instruction::RequestTide { client_seed: seed },
        riptide::accounts::RequestTide {
            payer: as_pubkey(&payer.pubkey()),
            market: round.market,
            oracle_queue: DEFAULT_EPHEMERAL_QUEUE,
            program_identity,
            vrf_program: VRF_PROGRAM_ID,
            slot_hashes: SLOT_HASHES,
            system_program: system_program::ID,
        },
    );
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[payer], request, "request_tide");
    fulfill(&mut round.svm, 1);
}

#[test]
fn vrf_moves_the_tide_and_closes_the_round() {
    let mut round = open_round(5);
    let keeper = Keypair::new();
    airdrop(&mut round.svm, &keeper.pubkey(), LAMPORTS_PER_SOL);

    let mut prices = vec![OPEN_PRICE];
    for tick in 1..=5u8 {
        tide(&mut round, &keeper, tick);
        let m = market_state(&round.svm, TransactionTarget::Ephemeral, round.market);
        assert_eq!(m.tick, tick as u32);
        assert_eq!(m.history[tick as usize], m.price);
        assert!(m.high >= m.price && m.low <= m.price);
        prices.push(m.price);
    }
    // Randomness actually moved the price.
    assert!(prices.windows(2).any(|w| w[0] != w[1]), "{prices:?}");
    let m = market_state(&round.svm, TransactionTarget::Ephemeral, round.market);
    assert_eq!(m.status, STATUS_CLOSED);
    assert_ne!(m.last_randomness, [0u8; 32]);

    // A closed round refuses new randomness requests.
    let (program_identity, _) = Pubkey::find_program_address(&[b"identity"], &round.program);
    let request = ix(
        round.program,
        riptide::instruction::RequestTide { client_seed: 9 },
        riptide::accounts::RequestTide {
            payer: as_pubkey(&keeper.pubkey()),
            market: round.market,
            oracle_queue: DEFAULT_EPHEMERAL_QUEUE,
            program_identity,
            vrf_program: VRF_PROGRAM_ID,
            slot_hashes: SLOT_HASHES,
            system_program: system_program::ID,
        },
    );
    let logs = send_fails(&mut round.svm, TransactionTarget::Ephemeral, &[&keeper], request);
    assert!(logs.iter().any(|l| l.contains("RoundClosed")), "{logs:#?}");
}

#[test]
fn full_round_trades_on_the_rollup_and_settles_on_solana() {
    let mut round = open_round(50);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let alice_t = add_trader(&mut round, &alice);
    let bob_t = add_trader(&mut round, &bob);

    let m = market_state(&round.svm, TransactionTarget::Ephemeral, round.market);
    assert_eq!(m.roster_len, 2);
    assert_eq!(m.roster[0], alice_t);
    assert_eq!(m.roster[1], bob_t);
    assert_eq!(m.status, STATUS_OPEN);

    // Alice goes long, Bob goes short, with the same size.
    let size = (2_000 * CHIP) as u64;
    let ix_a = trade_ix(&round, &alice, alice_t, 1, size);
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&alice], ix_a, "alice long");
    let ix_b = trade_ix(&round, &bob, bob_t, -1, size);
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&bob], ix_b, "bob short");

    // Leverage is capped at 5x equity.
    let too_big = (STARTING_CHIPS * MAX_LEVERAGE + CHIP) as u64;
    let bad = trade_ix(&round, &alice, alice_t, 1, too_big);
    let logs = send_fails(&mut round.svm, TransactionTarget::Ephemeral, &[&alice], bad);
    assert!(logs.iter().any(|l| l.contains("OverLeveraged")), "{logs:#?}");

    // Bob cannot trade Alice's account.
    let hijack = trade_ix(&round, &bob, alice_t, 0, 0);
    send_fails(&mut round.svm, TransactionTarget::Ephemeral, &[&bob], hijack);

    let keeper = Keypair::new();
    airdrop(&mut round.svm, &keeper.pubkey(), LAMPORTS_PER_SOL);
    for seed in 1..=8u8 {
        tide(&mut round, &keeper, seed);
    }
    let m = market_state(&round.svm, TransactionTarget::Ephemeral, round.market);
    assert_eq!(m.tick, 8);
    let price = m.price;

    // Alice goes flat: her PnL is realized at the live price.
    let a_before = trader_state(&round.svm, TransactionTarget::Ephemeral, alice_t);
    let expected_pnl =
        a_before.size as i128 * (price as i128 - a_before.entry_price as i128) / 1_000_000;
    let flat = trade_ix(&round, &alice, alice_t, 0, 0);
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&alice], flat, "alice flat");
    let a = trader_state(&round.svm, TransactionTarget::Ephemeral, alice_t);
    assert_eq!(a.size, 0);
    assert_eq!(a.cash as i128, STARTING_CHIPS as i128 + expected_pnl);
    assert_eq!(a.realized_pnl as i128, expected_pnl);
    assert_eq!(a.trades, 2);

    // Equal and opposite positions: Bob's open PnL mirrors Alice's.
    let b = trader_state(&round.svm, TransactionTarget::Ephemeral, bob_t);
    assert_eq!(b.unrealized(price).unwrap() as i128, -expected_pnl);

    // The authority ends the round; trading stops.
    let auth = as_pubkey(&round.authority.pubkey());
    let close = ix(
        round.program,
        riptide::instruction::CloseRound {},
        riptide::accounts::CloseRound { authority: auth, market: round.market },
    );
    let authority = round.authority.insecure_clone();
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&authority], close, "close_round");
    let late = trade_ix(&round, &alice, alice_t, 1, size);
    let logs = send_fails(&mut round.svm, TransactionTarget::Ephemeral, &[&alice], late);
    assert!(logs.iter().any(|l| l.contains("RoundClosed")), "{logs:#?}");

    // Bob cashes out: his short is closed and his chips land on Solana.
    let bob_pk = as_pubkey(&bob.pubkey());
    let cash_out = ix(
        round.program,
        riptide::instruction::CashOut {},
        riptide::accounts::CashOut {
            owner: bob_pk,
            market: round.market,
            trader: bob_t,
            magic_program: MagicProgram::id(),
            magic_context: MAGIC_CONTEXT_ID,
        },
    );
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&bob], cash_out, "bob cash_out");
    assert!(!is_delegated(&round.svm, &as_address(bob_t)));
    let b_base = trader_state(&round.svm, TransactionTarget::Base, bob_t);
    assert!(b_base.cashed_out);
    assert_eq!(b_base.size, 0);
    assert_eq!(b_base.cash as i128, STARTING_CHIPS as i128 - expected_pnl);

    // The market settles on Solana with the same final state as the rollup.
    let er_market = market_state(&round.svm, TransactionTarget::Ephemeral, round.market);
    let settle = ix(
        round.program,
        riptide::instruction::SettleMarket {},
        riptide::accounts::SettleMarket {
            authority: auth,
            market: round.market,
            magic_program: MagicProgram::id(),
            magic_context: MAGIC_CONTEXT_ID,
        },
    );
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&authority], settle, "settle_market");
    assert!(!is_delegated(&round.svm, &as_address(round.market)));
    let base_market = market_state(&round.svm, TransactionTarget::Base, round.market);
    assert_eq!(base_market.price, er_market.price);
    assert_eq!(base_market.tick, er_market.tick);
    assert_eq!(base_market.status, STATUS_CLOSED);
}

#[test]
fn wiped_out_positions_can_be_liquidated_by_anyone() {
    // A stormy market: one side of a max-leverage long/short pair must wipe out.
    let mut round = open_round_with(400, 300);
    let bull = Keypair::new();
    let bear = Keypair::new();
    let bull_t = add_trader(&mut round, &bull);
    let bear_t = add_trader(&mut round, &bear);
    let keeper = Keypair::new();
    airdrop(&mut round.svm, &keeper.pubkey(), 5 * LAMPORTS_PER_SOL);

    let max = (STARTING_CHIPS * MAX_LEVERAGE) as u64;
    let long = trade_ix(&round, &bull, bull_t, 1, max);
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&bull], long, "max long");
    let short = trade_ix(&round, &bear, bear_t, -1, max);
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&bear], short, "max short");

    let liq = |round: &Round, trader: Pubkey| {
        ix(
            round.program,
            riptide::instruction::Liquidate {},
            riptide::accounts::Liquidate { market: round.market, trader },
        )
    };
    // Healthy positions cannot be liquidated.
    let healthy = liq(&round, bull_t);
    let logs = send_fails(&mut round.svm, TransactionTarget::Ephemeral, &[&keeper], healthy);
    assert!(logs.iter().any(|l| l.contains("NotLiquidatable")), "{logs:#?}");

    let mut seed = 0u8;
    let victim = loop {
        let m = market_state(&round.svm, TransactionTarget::Ephemeral, round.market);
        assert_eq!(m.status, STATUS_OPEN, "no wipe-out in a whole stormy round");
        let bull_eq = trader_state(&round.svm, TransactionTarget::Ephemeral, bull_t)
            .equity(m.price)
            .unwrap();
        let bear_eq = trader_state(&round.svm, TransactionTarget::Ephemeral, bear_t)
            .equity(m.price)
            .unwrap();
        if bull_eq <= 0 {
            break bull_t;
        }
        if bear_eq <= 0 {
            break bear_t;
        }
        seed = seed.wrapping_add(1);
        tide(&mut round, &keeper, seed);
    };
    let wipe = liq(&round, victim);
    send_ix(&mut round.svm, TransactionTarget::Ephemeral, &[&keeper], wipe, "liquidate");
    let tr = trader_state(&round.svm, TransactionTarget::Ephemeral, victim);
    assert_eq!(tr.size, 0);
    assert_eq!(tr.cash, 0);
    assert_eq!(tr.realized_pnl, -STARTING_CHIPS);
}
