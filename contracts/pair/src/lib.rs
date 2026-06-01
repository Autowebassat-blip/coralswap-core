#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod dynamic_fee;
mod errors;
mod events;
mod fee_decay;
mod flash_loan;
mod math;
mod oracle;
mod reentrancy;
mod storage;

#[cfg(test)]
mod test;

use dynamic_fee::compute_fee_bps;
use errors::PairError;
use events::{PairEvents, PauseEvents};
use math::MINIMUM_LIQUIDITY;
use soroban_sdk::{contract, contractclient, contractimpl, token::TokenClient, Address, Env};
use storage::{
    get_fee_state, get_fee_stats_state, get_pair_state, set_fee_state, set_fee_stats_state,
    set_pair_state, set_reentrancy_guard, FeeState, FeeStats, ReentrancyGuard,
};

#[contractclient(name = "LpTokenClient")]
pub trait LpTokenInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn total_supply(env: Env) -> i128;
}

#[contract]
pub struct Pair;

#[contractimpl]
impl Pair {
    // ─────────────────────────────────────────
    // Initialize
    // ─────────────────────────────────────────

    pub fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
        fee_bps: u32,
        admin: Address,
    ) -> Result<(), PairError> {
        // 1. Double-init guard
        if get_pair_state(&env).is_some() {
            return Err(PairError::AlreadyInitialized);
        }

        // 2. Identical-token guard
        if token_a == token_b {
            return Err(PairError::InvalidInput);
        }

        let state = storage::PairStorage {
            factory,
            token_a,
            token_b,
            lp_token,
            reserve_a: 0,
            reserve_b: 0,
            block_timestamp_last: env.ledger().timestamp(),
            price_a_cumulative: 0,
            price_b_cumulative: 0,
            k_last: 0,
            fee_bps,
            admin,
            paused: false,
        };

        set_pair_state(&env, &state);

        // 4. Initialize FeeState — use the tier's fee_bps as baseline.
        // min = baseline / 3 (at least 1); max = baseline * 10 / 3
        // For standard 30 bps: min=10, max=100.
        let fee_state = FeeState {
            vol_accumulator: 0,
            ema_alpha: 10_000_000_000_000, // 10% of SCALE (1e14)
            baseline_fee_bps: fee_bps,
            min_fee_bps: (fee_bps / 3).max(1),
            max_fee_bps: fee_bps.saturating_mul(10) / 3,
            ramp_up_multiplier: 2,
            cooldown_divisor: 2,
            last_fee_update: 0,
            decay_threshold_blocks: 100,
        };
        set_fee_state(&env, &fee_state);

        // 5. Initialize ReentrancyGuard as unlocked
        set_reentrancy_guard(&env, &ReentrancyGuard { locked: false });

        // 6. Initialize FeeStatsState with zeroes
        set_fee_stats_state(
            &env,
            &storage::FeeStatsState {
                fees_collected_0: 0,
                fees_collected_1: 0,
                window_start: env.ledger().timestamp(),
                volume_current: 0,
                volume_previous: 0,
            },
        );

        // 7. Extend instance storage TTL (~7 days at 5s/ledger)
        const TTL_THRESHOLD: u32 = 60_480;
        const TTL_EXTEND_TO: u32 = 120_960;
        env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);

        Ok(())
    }

    // ─────────────────────────────────────────
    // Mint
    // ─────────────────────────────────────────

    pub fn mint(env: Env, to: Address) -> Result<i128, PairError> {
        to.require_auth();

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        if state.paused {
            return Err(PairError::Paused);
        }
        let contract = env.current_contract_address();

        let balance_a = TokenClient::new(&env, &state.token_a).balance(&contract);
        let balance_b = TokenClient::new(&env, &state.token_b).balance(&contract);

        let amount_a = balance_a.checked_sub(state.reserve_a).ok_or(PairError::InvalidInput)?;

        let amount_b = balance_b.checked_sub(state.reserve_b).ok_or(PairError::InvalidInput)?;

        let lp_client = LpTokenClient::new(&env, &state.lp_token);
        let total_supply = lp_client.total_supply();

        let liquidity;

        if total_supply == 0 {
            let product = amount_a.checked_mul(amount_b).ok_or(PairError::Overflow)?;

            liquidity = math::sqrt(product)
                .checked_sub(MINIMUM_LIQUIDITY)
                .ok_or(PairError::InsufficientLiquidityMinted)?;

            lp_client.mint(&contract, &MINIMUM_LIQUIDITY);
        } else {
            let liquidity_a = amount_a
                .checked_mul(total_supply)
                .ok_or(PairError::Overflow)?
                .checked_div(state.reserve_a)
                .ok_or(PairError::Overflow)?;

            let liquidity_b = amount_b
                .checked_mul(total_supply)
                .ok_or(PairError::Overflow)?
                .checked_div(state.reserve_b)
                .ok_or(PairError::Overflow)?;

            liquidity = liquidity_a.min(liquidity_b);
        }

        if liquidity <= 0 {
            return Err(PairError::InsufficientLiquidityMinted);
        }

        lp_client.mint(&to, &liquidity);

        state.reserve_a = balance_a;
        state.reserve_b = balance_b;
        state.k_last = balance_a.checked_mul(balance_b).ok_or(PairError::Overflow)?;

        state.block_timestamp_last = env.ledger().timestamp();

        set_pair_state(&env, &state);

        PairEvents::mint(&env, &to, amount_a, amount_b);

        Ok(liquidity)
    }

    // ─────────────────────────────────────────
    // Burn
    // ─────────────────────────────────────────

    pub fn burn(env: Env, to: Address) -> Result<(i128, i128), PairError> {
        to.require_auth();

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        if state.paused {
            return Err(PairError::Paused);
        }
        let contract = env.current_contract_address();

        let lp_balance = TokenClient::new(&env, &state.lp_token).balance(&contract);

        let total_supply = LpTokenClient::new(&env, &state.lp_token).total_supply();

        if total_supply == 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        let amount_a = lp_balance
            .checked_mul(state.reserve_a)
            .ok_or(PairError::Overflow)?
            .checked_div(total_supply)
            .ok_or(PairError::Overflow)?;

        let amount_b = lp_balance
            .checked_mul(state.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_div(total_supply)
            .ok_or(PairError::Overflow)?;

        if amount_a <= 0 || amount_b <= 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        LpTokenClient::new(&env, &state.lp_token).burn(&contract, &lp_balance);

        TokenClient::new(&env, &state.token_a).transfer(&contract, &to, &amount_a);

        TokenClient::new(&env, &state.token_b).transfer(&contract, &to, &amount_b);

        state.reserve_a = state.reserve_a.checked_sub(amount_a).ok_or(PairError::Overflow)?;

        state.reserve_b = state.reserve_b.checked_sub(amount_b).ok_or(PairError::Overflow)?;

        state.k_last = state.reserve_a.checked_mul(state.reserve_b).ok_or(PairError::Overflow)?;

        state.block_timestamp_last = env.ledger().timestamp();

        set_pair_state(&env, &state);

        PairEvents::burn(&env, &to, amount_a, amount_b, &to);

        Ok((amount_a, amount_b))
    }

    // ─────────────────────────────────────────
    // Swap
    // ─────────────────────────────────────────

    pub fn swap(
        env: Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: Address,
    ) -> Result<(), PairError> {
        let _guard = reentrancy::ReentrancyGuard::acquire(&env)?;
        Self::swap_inner(&env, amount_a_out, amount_b_out, &to)
    }

    // ─────────────────────────────────────────
    // Views
    // ─────────────────────────────────────────

    /// Returns (reserve_a, reserve_b, block_timestamp_last).
    pub fn get_reserves(env: Env) -> Result<(i128, i128, u64), PairError> {
        let state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        Ok((state.reserve_a, state.reserve_b, state.block_timestamp_last))
    }

    /// Consults the oracle for a TWAP over a given window.
    pub fn consult_twap(env: Env, window_ledgers: u32) -> Result<(i128, i128), errors::OracleError> {
        oracle::consult_twap(&env, window_ledgers)
    }

    /// Returns the LP token address.
    pub fn lp_token(env: Env) -> Result<Address, PairError> {
        let state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        Ok(state.lp_token)
    }

    /// Returns the current dynamic fee in basis points.
    pub fn get_current_fee_bps(env: Env) -> u32 {
        match get_fee_state(&env) {
            Some(fs) => compute_fee_bps(&fs),
            None => 30,
        }
    }

    /// Returns aggregate fee and volume statistics (read-only, no state change).
    pub fn get_fee_stats(env: Env) -> FeeStats {
        let stats = get_fee_stats_state(&env);
        let fee_bps = match get_fee_state(&env) {
            Some(fs) => compute_fee_bps(&fs),
            None => 30,
        };

        let now = env.ledger().timestamp();
        const WINDOW: u64 = 86_400;

        let volume_24h = if now.saturating_sub(stats.window_start) < WINDOW {
            // Within current window: blend with proportion of previous window still in range.
            let elapsed = now.saturating_sub(stats.window_start);
            let prev_weight = (WINDOW - elapsed) as i128;
            let prev_contribution = stats
                .volume_previous
                .saturating_mul(prev_weight)
                .checked_div(WINDOW as i128)
                .unwrap_or(0);
            stats.volume_current.saturating_add(prev_contribution)
        } else {
            // Current window has expired; no meaningful data.
            0
        };

        FeeStats { volume_24h, fees_collected_0: stats.fees_collected_0, fees_collected_1: stats.fees_collected_1, fee_bps }
    }

    // ─────────────────────────────────────────
    // Admin: pause / unpause
    // ─────────────────────────────────────────

    /// Pauses all state-mutating operations. Only callable by the pair admin.
    pub fn pause(env: Env, by: Address) -> Result<(), PairError> {
        by.require_auth();
        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        if by != state.admin {
            return Err(PairError::Unauthorized);
        }
        state.paused = true;
        set_pair_state(&env, &state);
        PauseEvents::paused(&env, &by, env.ledger().sequence());
        Ok(())
    }

    /// Resumes all state-mutating operations. Only callable by the pair admin.
    pub fn unpause(env: Env, by: Address) -> Result<(), PairError> {
        by.require_auth();
        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        if by != state.admin {
            return Err(PairError::Unauthorized);
        }
        state.paused = false;
        set_pair_state(&env, &state);
        PauseEvents::unpaused(&env, &by, env.ledger().sequence());
        Ok(())
    }

    /// Returns whether this pair is currently paused.
    pub fn is_paused(env: Env) -> bool {
        get_pair_state(&env).map(|s| s.paused).unwrap_or(false)
    }

    // ─────────────────────────────────────────
    // Sync
    // ─────────────────────────────────────────

    /// Syncs reserves to actual token balances and updates the oracle timestamp.
    pub fn sync(env: Env) -> Result<(), PairError> {
        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();

        let balance_a = TokenClient::new(&env, &state.token_a).balance(&contract);
        let balance_b = TokenClient::new(&env, &state.token_b).balance(&contract);

        state.reserve_a = balance_a;
        state.reserve_b = balance_b;
        state.block_timestamp_last = env.ledger().timestamp();
        state.k_last = balance_a.checked_mul(balance_b).ok_or(PairError::Overflow)?;

        set_pair_state(&env, &state);

        PairEvents::sync(&env, balance_a, balance_b);

        Ok(())
    }

    fn swap_inner(
        env: &Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: &Address,
    ) -> Result<(), PairError> {
        if amount_a_out <= 0 && amount_b_out <= 0 {
            return Err(PairError::InsufficientOutputAmount);
        }

        let mut pair = get_pair_state(env).ok_or(PairError::NotInitialized)?;
        if pair.paused {
            return Err(PairError::Paused);
        }
        let mut fee_state = get_fee_state(env).ok_or(PairError::NotInitialized)?;

        if amount_a_out >= pair.reserve_a || amount_b_out >= pair.reserve_b {
            return Err(PairError::InsufficientLiquidity);
        }

        // Store pre-swap reserves for price delta calculation
        let reserve_a_before = pair.reserve_a;
        let reserve_b_before = pair.reserve_b;

        dynamic_fee::decay_stale_ema(env, &mut fee_state);
        let fee_bps = dynamic_fee::compute_fee_bps(&fee_state);

        let contract_address = env.current_contract_address();

        if amount_a_out > 0 {
            TokenClient::new(env, &pair.token_a).transfer(&contract_address, to, &amount_a_out);
        }

        if amount_b_out > 0 {
            TokenClient::new(env, &pair.token_b).transfer(&contract_address, to, &amount_b_out);
        }

        let balance_a = TokenClient::new(env, &pair.token_a).balance(&contract_address);

        let balance_b = TokenClient::new(env, &pair.token_b).balance(&contract_address);

        let amount_a_in = (balance_a - (pair.reserve_a - amount_a_out)).max(0);

        let amount_b_in = (balance_b - (pair.reserve_b - amount_b_out)).max(0);

        if amount_a_in <= 0 && amount_b_in <= 0 {
            return Err(PairError::InsufficientInputAmount);
        }

        let fee = fee_bps as i128;

        let balance_a_adj = balance_a
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(amount_a_in * fee)
            .ok_or(PairError::Overflow)?;

        let balance_b_adj = balance_b
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(amount_b_in * fee)
            .ok_or(PairError::Overflow)?;

        let k_before = pair
            .reserve_a
            .checked_mul(pair.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_mul(100_000_000)
            .ok_or(PairError::Overflow)?;

        let k_after = balance_a_adj.checked_mul(balance_b_adj).ok_or(PairError::Overflow)?;

        if k_after < k_before {
            return Err(PairError::InvalidK);
        }

        pair.reserve_a = balance_a;
        pair.reserve_b = balance_b;
        pair.k_last = balance_a.checked_mul(balance_b).ok_or(PairError::Overflow)?;

        pair.block_timestamp_last = env.ledger().timestamp();

        // --- Update volatility tracking after reserves change ---
        // Compute price delta as the absolute change in the price ratio.
        // price_before = reserve_b_before / reserve_a_before (scaled)
        // price_after  = reserve_b / reserve_a (scaled)
        // price_delta  = |price_after - price_before| (scaled)
        const SCALE: i128 = 100_000_000_000_000; // 1e14, matches dynamic_fee::SCALE
        
        if reserve_a_before > 0 && reserve_b_before > 0 && pair.reserve_a > 0 && pair.reserve_b > 0 {
            // price_before = (reserve_b_before * SCALE) / reserve_a_before
            let price_before = reserve_b_before
                .checked_mul(SCALE)
                .and_then(|v| v.checked_div(reserve_a_before))
                .unwrap_or(0);
            
            // price_after = (reserve_b * SCALE) / reserve_a
            let price_after = pair.reserve_b
                .checked_mul(SCALE)
                .and_then(|v| v.checked_div(pair.reserve_a))
                .unwrap_or(0);
            
            // price_delta_abs = |price_after - price_before|
            let price_delta_abs = if price_after > price_before {
                price_after - price_before
            } else {
                price_before - price_after
            };
            
            // trade_size = total input amount (in terms of reserve A equivalent)
            // For simplicity, use the larger of the two inputs
            let trade_size = amount_a_in.max(amount_b_in);
            
            // total_reserve = reserve_a + reserve_b (simple sum for size weighting)
            let total_reserve = pair.reserve_a.saturating_add(pair.reserve_b);
            
            // Update volatility (ignore errors to not break swaps on edge cases)
            let _ = dynamic_fee::update_volatility(
                env,
                &mut fee_state,
                price_delta_abs,
                trade_size,
                total_reserve,
            );
        }

        // Update fee stats: accumulate fees collected and rolling 24h volume.
        {
            const WINDOW: u64 = 86_400;
            let now = pair.block_timestamp_last;
            let mut stats = get_fee_stats_state(env);

            // Roll the window if 24 h have elapsed.
            if now.saturating_sub(stats.window_start) >= WINDOW {
                stats.volume_previous = stats.volume_current;
                stats.volume_current = 0;
                stats.window_start = now;
            }

            // Volume = total input routed through this swap.
            let trade_volume = amount_a_in.saturating_add(amount_b_in);
            stats.volume_current = stats.volume_current.saturating_add(trade_volume);

            // Fees collected = input * fee_bps / 10_000 per token.
            let fee_a = amount_a_in
                .checked_mul(fee_bps as i128)
                .map(|v| v / 10_000)
                .unwrap_or(0);
            let fee_b = amount_b_in
                .checked_mul(fee_bps as i128)
                .map(|v| v / 10_000)
                .unwrap_or(0);
            stats.fees_collected_0 = stats.fees_collected_0.saturating_add(fee_a);
            stats.fees_collected_1 = stats.fees_collected_1.saturating_add(fee_b);

            set_fee_stats_state(env, &stats);
        }

        set_pair_state(env, &pair);
        set_fee_state(env, &fee_state);

        PairEvents::swap(
            env,
            to,
            amount_a_in,
            amount_b_in,
            amount_a_out,
            amount_b_out,
            fee_bps,
            to,
        );

        Ok(())
    }
}
