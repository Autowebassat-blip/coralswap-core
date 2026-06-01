#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod helpers;
mod storage;

#[cfg(test)]
mod test;

use errors::RouterError;
use helpers::{compute_optimal_amounts, get_pair_address, FeeTier, PairClient};
use soroban_sdk::{contract, contractimpl, contracttype, token::TokenClient, Address, Env, Vec};
use storage::{get_factory, set_factory};

/// Result returned by `simulate_swap`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SimulationResult {
    /// Expected output amount in the output token's stroops.
    pub amount_out: i128,
    /// Price impact in basis points (deviation from current spot price).
    pub price_impact_bps: u32,
    /// Fee taken from the input amount (in input token stroops).
    pub fee_amount: i128,
    /// Execution price: amount_out * 1e14 / amount_in (fixed-point).
    pub execution_price: i128,
}

#[contract]
pub struct Router;

#[contractimpl]
impl Router {
    pub fn initialize(env: Env, factory: Address) {
        set_factory(&env, &factory);
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
        fee_tier: FeeTier,
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
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b, fee_tier)?;

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
        fee_tier: FeeTier,
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
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b, fee_tier)?;

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

    /// Simulates a swap and returns the expected output without mutating state.
    ///
    /// Uses the constant-product formula with the pair's current fee to
    /// compute the same result that `Pair::swap` would produce for the same
    /// reserve state.
    ///
    /// Returns `RouterError::InsufficientLiquidity` when the pair has no
    /// reserves or the requested input would drain the pool.
    pub fn simulate_swap(
        env: Env,
        token_in: Address,
        token_out: Address,
        fee_tier: FeeTier,
        amount_in: i128,
    ) -> Result<SimulationResult, RouterError> {
        if amount_in <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let pair_address =
            get_pair_address(&env, &factory, &token_in, &token_out, fee_tier.clone())?;

        let pair_client = PairClient::new(&env, &pair_address);
        let (reserve_a, reserve_b, _) = pair_client.get_reserves();

        if reserve_a <= 0 || reserve_b <= 0 {
            return Err(RouterError::InsufficientLiquidity);
        }

        // Determine which reserve corresponds to token_in vs token_out.
        // The pair sorts tokens canonically at creation; token_a < token_b.
        // We use the fee tier bps directly (same formula the pair uses).
        let fee_bps = fee_tier.fee_bps();

        // Pair stores token_a (smaller address) as reserve_a.
        // Determine direction by comparing addresses.
        let (reserve_in, reserve_out) = if token_in < token_out {
            (reserve_a, reserve_b)
        } else {
            (reserve_b, reserve_a)
        };

        // fee_amount = amount_in * fee_bps / 10_000
        let fee_amount = amount_in
            .checked_mul(fee_bps as i128)
            .ok_or(RouterError::InsufficientLiquidity)?
            / 10_000;

        let amount_in_after_fee = amount_in - fee_amount;

        // Constant-product: amount_out = amount_in_after_fee * reserve_out / (reserve_in + amount_in_after_fee)
        let numerator = amount_in_after_fee
            .checked_mul(reserve_out)
            .ok_or(RouterError::InsufficientLiquidity)?;

        let denominator = reserve_in
            .checked_add(amount_in_after_fee)
            .ok_or(RouterError::InsufficientLiquidity)?;

        if denominator <= 0 {
            return Err(RouterError::InsufficientLiquidity);
        }

        let amount_out = numerator / denominator;

        if amount_out <= 0 || amount_out >= reserve_out {
            return Err(RouterError::InsufficientLiquidity);
        }

        // Fixed-point scale (1e14) for price calculations.
        const SCALE: i128 = 100_000_000_000_000;

        // spot_price = reserve_out * SCALE / reserve_in
        let spot_price = reserve_out
            .checked_mul(SCALE)
            .ok_or(RouterError::InsufficientLiquidity)?
            / reserve_in;

        // execution_price = amount_out * SCALE / amount_in
        let execution_price = amount_out
            .checked_mul(SCALE)
            .ok_or(RouterError::InsufficientLiquidity)?
            / amount_in;

        // price_impact_bps = |spot_price - execution_price| * 10_000 / spot_price
        let price_delta = if spot_price > execution_price {
            spot_price - execution_price
        } else {
            execution_price - spot_price
        };
        let price_impact_bps = (price_delta
            .checked_mul(10_000)
            .ok_or(RouterError::InsufficientLiquidity)?
            / spot_price) as u32;

        Ok(SimulationResult { amount_out, price_impact_bps, fee_amount, execution_price })
    }
}
