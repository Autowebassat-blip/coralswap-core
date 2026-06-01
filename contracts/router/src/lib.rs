#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod helpers;
mod price_guard;
mod storage;

#[cfg(test)]
mod test;

use errors::RouterError;
use helpers::{compute_optimal_amounts, get_pair_address, PairClient};
use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Bytes, Env, Vec};
use storage::{
    get_admin, get_factory, get_price_guard_config, set_admin, set_factory,
    set_price_guard_config, PriceGuardConfig,
};

#[contract]
pub struct Router;

#[contractimpl]
impl Router {
    pub fn initialize(env: Env, factory: Address) {
        set_factory(&env, &factory);
    }

    /// Initializes the Router with a factory and an explicit admin address.
    pub fn initialize_with_admin(env: Env, factory: Address, admin: Address) {
        set_factory(&env, &factory);
        set_admin(&env, &admin);
    }

    /// Updates the price guard configuration. Only callable by the admin.
    pub fn set_price_guard_config(
        env: Env,
        min_guarded_amount: i128,
        max_deviation_bps: u32,
    ) -> Result<(), RouterError> {
        let admin = get_admin(&env).ok_or(RouterError::Unauthorized)?;
        admin.require_auth();
        set_price_guard_config(
            &env,
            &PriceGuardConfig { min_guarded_amount, max_deviation_bps },
        );
        Ok(())
    }

    /// Swaps an exact `amount_in` of `path[0]` for at least `min_out` of `path[1]`,
    /// with an optional RedStone oracle price guard.
    ///
    /// If `redstone_payload` is `Some` **and** `amount_in >= config.min_guarded_amount`,
    /// the payload is validated for freshness and the execution price is checked against
    /// the oracle price. The payload encodes `(price_scaled: u128, timestamp: u64)` in
    /// 24 big-endian bytes (see `price_guard::PAYLOAD_LEN`).
    ///
    /// Reverts with:
    /// - `RouterError::StaleOraclePayload`       — payload older than 5 minutes
    /// - `RouterError::PriceDeviationTooHigh`    — execution price deviates > max_deviation_bps
    /// - `RouterError::Expired`                  — deadline passed
    /// - `RouterError::InsufficientOutputAmount` — amount_out < min_out
    pub fn swap_with_price_guard(
        env: Env,
        amount_in: i128,
        min_out: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
        redstone_payload: Option<Bytes>,
    ) -> Result<i128, RouterError> {
        // ── 1. Deadline ───────────────────────────────────────────────────────
        if deadline < env.ledger().timestamp() {
            return Err(RouterError::Expired);
        }
        if amount_in <= 0 {
            return Err(RouterError::ZeroAmount);
        }
        if path.len() < 2 {
            return Err(RouterError::InvalidPath);
        }

        let token_in = path.get(0).ok_or(RouterError::InvalidPath)?;
        let token_out = path.get(1).ok_or(RouterError::InvalidPath)?;

        if token_in == token_out {
            return Err(RouterError::IdenticalTokens);
        }

        // ── 2. Fetch pair and reserves ────────────────────────────────────────
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let pair_addr = get_pair_address(&env, &factory, &token_in, &token_out)?;
        let pair_client = PairClient::new(&env, &pair_addr);
        let (reserve_a, reserve_b, _) = pair_client.get_reserves();
        let fee_bps = pair_client.get_current_fee_bps();

        // Determine which reserve is in/out based on token sort order.
        let (reserve_in, reserve_out) = if token_in < token_out {
            (reserve_a, reserve_b)
        } else {
            (reserve_b, reserve_a)
        };

        if reserve_in <= 0 || reserve_out <= 0 {
            return Err(RouterError::InsufficientLiquidity);
        }

        // ── 3. Compute amount_out ─────────────────────────────────────────────
        let amount_out =
            helpers::get_amount_out(&env, amount_in, reserve_in, reserve_out, fee_bps)?;

        if amount_out < min_out {
            return Err(RouterError::InsufficientOutputAmount);
        }

        // ── 4. Oracle price guard ─────────────────────────────────────────────
        let config = get_price_guard_config(&env);
        let guarded = config
            .as_ref()
            .map(|c| amount_in >= c.min_guarded_amount)
            .unwrap_or(false);

        if guarded {
            let payload = redstone_payload.ok_or(RouterError::InvalidOraclePayload)?;
            let cfg = config.unwrap(); // safe: guarded == true implies config is Some

            let (oracle_price_scaled, payload_ts) = price_guard::parse_payload(&payload)?;
            price_guard::check_freshness(&env, payload_ts)?;

            // Execution price: amount_out / amount_in, scaled to 10^8.
            let exec_price_scaled =
                ((amount_out as u128) * 100_000_000) / (amount_in as u128);

            price_guard::check_deviation(
                exec_price_scaled,
                oracle_price_scaled,
                cfg.max_deviation_bps,
            )?;
        }

        // ── 5. Execute swap ───────────────────────────────────────────────────
        to.require_auth();
        TokenClient::new(&env, &token_in).transfer(&to, &pair_addr, &amount_in);

        let (amount_a_out, amount_b_out) = if token_in < token_out {
            (0_i128, amount_out)
        } else {
            (amount_out, 0_i128)
        };
        pair_client.swap(&amount_a_out, &amount_b_out, &to);

        Ok(amount_out)
    }
    pub fn swap_exact_tokens_for_tokens(
        _env: Env,
        _amount_in: i128,
        _amount_out_min: i128,
        _path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        todo!()
    }

    /// Swaps tokens to receive an exact amount of output tokens (not yet implemented).
    ///
    /// # Arguments
    /// * `amount_out` - The exact amount of output tokens desired
    /// * `amount_in_max` - The maximum amount of input tokens to spend
    /// * `path` - Vector of token addresses representing the swap route
    /// * `to` - The recipient address for output tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn swap_tokens_for_exact_tokens(
        _env: Env,
        _amount_out: i128,
        _amount_in_max: i128,
        _path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        todo!()
    }

    /// Adds liquidity to a token pair (not yet implemented).
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `amount_a_desired` - Desired amount of token_a to add
    /// * `amount_b_desired` - Desired amount of token_b to add
    /// * `amount_a_min` - Minimum amount of token_a to add
    /// * `amount_b_min` - Minimum amount of token_b to add
    /// * `to` - Recipient of LP tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn add_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> Result<(i128, i128, i128), RouterError> {
        // Check deadline
        if deadline < env.ledger().timestamp() {
            return Err(RouterError::Expired);
        }

        // Validate inputs: reject zero desired amounts
        if amount_a_desired <= 0 || amount_b_desired <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        // Validate inputs: reject identical tokens
        if token_a == token_b {
            return Err(RouterError::IdenticalTokens);
        }

        // Get factory address
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Get pair address from factory
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b)?;

        // Get pair contract client and current reserves
        let pair_client = PairClient::new(&env, &pair_address);
        let (reserve_a, reserve_b, _) = pair_client.get_reserves();

        // Calculate optimal deposit amounts preserving pool ratio
        let (amount_a, amount_b) = compute_optimal_amounts(
            amount_a_desired,
            amount_b_desired,
            amount_a_min,
            amount_b_min,
            reserve_a,
            reserve_b,
        )?;

        // The user must provide authorization for token transfers
        to.require_auth();

        // Transfer tokens from 'to' to the pair contract
        TokenClient::new(&env, &token_a).transfer(&to, &pair_address, &amount_a);
        TokenClient::new(&env, &token_b).transfer(&to, &pair_address, &amount_b);

        // Mint LP tokens to the recipient
        let liquidity = pair_client.mint(&to);

        Ok((amount_a, amount_b, liquidity))
    }

    /// Removes liquidity from a token pair (not yet implemented).
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `liquidity` - Amount of LP tokens to burn
    /// * `amount_a_min` - Minimum amount of token_a to receive
    /// * `amount_b_min` - Minimum amount of token_b to receive
    /// * `to` - Recipient of underlying tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn remove_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        liquidity: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> Result<(i128, i128), RouterError> {
        // Check deadline
        if deadline < env.ledger().timestamp() {
            return Err(RouterError::Expired);
        }

        // Check for non-zero liquidity
        if liquidity <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        // Check for identical tokens
        if token_a == token_b {
            return Err(RouterError::IdenticalTokens);
        }

        // Get factory address
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Get pair address
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b)?;

        // Get pair contract client
        let pair_client = PairClient::new(&env, &pair_address);

        // Get LP token address from pair
        let lp_token_address = pair_client.lp_token();

        // The user must provide authorization for the Router to transfer LP tokens
        to.require_auth();

        // Transfer LP tokens from 'to' to pair
        let lp_token_client = TokenClient::new(&env, &lp_token_address);
        lp_token_client.transfer(&to, &pair_address, &liquidity);

        // Call Pair::burn(to) - this will burn LP tokens from the pair and transfer underlying tokens
        let (amount_a, amount_b) = pair_client.burn(&to);

        // Enforce minimum output amounts
        if amount_a < amount_a_min || amount_b < amount_b_min {
            return Err(RouterError::InsufficientOutputAmount);
        }

        Ok((amount_a, amount_b))
    }
}
