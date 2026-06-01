use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

mod price_guard;

// ── Mock Contracts ────────────────────────────────────────────────────────────

/// Minimal mock factory that stores and returns a pair address.
#[contract]
pub struct MockFactory;

#[contracttype]
#[derive(Clone)]
pub enum MFKey {
    Pair(Address, Address),
}

#[contractimpl]
impl MockFactory {
    /// Pre-register a pair address for a token pair (sorted canonically).
    pub fn set_pair(env: Env, token_a: Address, token_b: Address, pair: Address) {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().set(&MFKey::Pair(t0, t1), &pair);
    }

    /// Get a registered pair; returns None if not set.
    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().get(&MFKey::Pair(t0, t1))
    }

    /// "Create" a pair — in tests, just returns the pre-registered address.
    /// Tests must call set_pair before triggering any path that calls create_pair.
    pub fn create_pair(env: Env, token_a: Address, token_b: Address) -> Address {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage()
            .instance()
            .get(&MFKey::Pair(t0, t1))
            .expect("test must call set_pair before create_pair is invoked")
    }
}

/// Minimal mock pair that returns pre-configured amounts and supports all PairClient methods.

#[contract]
pub struct MockPair;

#[contracttype]
#[derive(Clone)]
pub enum MPKey {
    ReserveA,
    ReserveB,
    MintReturn,
    FeeBps,

    BurnAmountA,
    BurnAmountB,
    LiquidityToMint,
}

#[contractimpl]
impl MockPair {
    pub fn set_reserves(env: Env, reserve_a: i128, reserve_b: i128) {
        env.storage().instance().set(&MPKey::ReserveA, &reserve_a);
        env.storage().instance().set(&MPKey::ReserveB, &reserve_b);
    }

    pub fn get_reserves(env: Env) -> (i128, i128, u64) {
        let a: i128 = env.storage().instance().get(&MPKey::ReserveA).unwrap_or(0);

        let b: i128 = env.storage().instance().get(&MPKey::ReserveB).unwrap_or(0);

        (a, b, 0)
    }

    pub fn set_fee_bps(env: Env, fee_bps: u32) {
        env.storage().instance().set(&MPKey::FeeBps, &fee_bps);
    }

    pub fn lp_token(env: Env) -> Address {
        env.storage().instance().get(&MPKey::LpToken).unwrap()
    pub fn set_burn_amounts(env: Env, amount_a: i128, amount_b: i128) {
        env.storage().instance().set(&MPKey::BurnAmountA, &amount_a);
        env.storage().instance().set(&MPKey::BurnAmountB, &amount_b);
    }

    pub fn burn(env: Env, _to: Address) -> (i128, i128) {
        let a: i128 = env.storage().instance().get(&MPKey::BurnAmountA).unwrap_or(0);

    pub fn set_mint_return(env: Env, liquidity: i128) {
        env.storage().instance().set(&MPKey::MintReturn, &liquidity);
        let b: i128 = env.storage().instance().get(&MPKey::BurnAmountB).unwrap_or(0);

        (a, b)
    }

    pub fn set_liquidity_to_mint(env: Env, liquidity: i128) {
        env.storage().instance().set(&MPKey::LiquidityToMint, &liquidity);
    }

    pub fn get_current_fee_bps(env: Env) -> u32 {
        env.storage().instance().get(&MPKey::FeeBps).unwrap_or(30)
    }

    pub fn mint(env: Env, _to: Address) -> i128 {
        env.storage().instance().get(&MPKey::LiquidityToMint).unwrap_or(0)
    }

    pub fn swap(_env: Env, _amount_a_out: i128, _amount_b_out: i128, _to: Address) {}
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Sets up a full mock environment with Router, Factory, Pair, and tokens.
///
/// Returns (env, router_client, token_a, token_b, to, deadline,
///          mock_pair_client, lp_token_addr, pair_addr).
#[allow(clippy::type_complexity)]
fn setup_full_env() -> (
    Env,
    RouterClient<'static>,
    Address,
    Address,
    Address,
    u64,
    MockPairClient<'static>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    // Register contracts
    let router_addr = env.register_contract(None, Router);
    let router_client = RouterClient::new(&env, &router_addr);

    let factory_addr = env.register_contract(None, MockFactory);
    let mock_factory_client = MockFactoryClient::new(&env, &factory_addr);

    let pair_addr = env.register_contract(None, MockPair);
    let mock_pair_client = MockPairClient::new(&env, &pair_addr);

    // Create real Stellar Asset Contracts for LP and tokens
    let lp_admin = Address::generate(&env);
    let lp_token_addr = env.register_stellar_asset_contract_v2(lp_admin.clone()).address();
    let lp_sac_client = StellarAssetClient::new(&env, &lp_token_addr);

    let token_admin = Address::generate(&env);
    let token_a = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let token_b = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    let to = Address::generate(&env);

    // Wire up: Router → Factory → Pair → LP Token
    router_client.initialize(&factory_addr);
    mock_factory_client.set_pair(&token_a, &token_b, &pair_addr);
    mock_pair_client.set_lp_token(&lp_token_addr);
    mock_pair_client.set_burn_amounts(&500, &1000);

    // Mint tokens to recipient
    StellarAssetClient::new(&env, &token_a).mint(&to, &10000);
    StellarAssetClient::new(&env, &token_b).mint(&to, &10000);
    lp_sac_client.mint(&to, &2000);

    let deadline = env.ledger().timestamp() + 1000;

    (env, router_client, token_a, token_b, to, deadline, mock_pair_client, lp_token_addr, pair_addr)
}

// ── add_liquidity test setup ──────────────────────────────────────────────────

/// Sets up a full mock environment for add_liquidity tests with real token contracts.
///
/// Returns (env, router_client, token_a, token_b, to, deadline,
///          mock_pair_client, pair_addr).
#[allow(clippy::type_complexity)]
fn setup_add_liquidity_env(
) -> (Env, RouterClient<'static>, Address, Address, Address, u64, MockPairClient<'static>, Address)
{
    let env = Env::default();
    env.mock_all_auths();

    // Register contracts
    let router_addr = env.register_contract(None, Router);
    let router_client = RouterClient::new(&env, &router_addr);

    let factory_addr = env.register_contract(None, MockFactory);
    let mock_factory_client = MockFactoryClient::new(&env, &factory_addr);

    let pair_addr = env.register_contract(None, MockPair);
    let mock_pair_client = MockPairClient::new(&env, &pair_addr);

    // Create real token contracts for token_a and token_b
    let admin_a = Address::generate(&env);
    let token_a = env.register_stellar_asset_contract_v2(admin_a).address();
    let sac_a = StellarAssetClient::new(&env, &token_a);

    let admin_b = Address::generate(&env);
    let token_b = env.register_stellar_asset_contract_v2(admin_b).address();
    let sac_b = StellarAssetClient::new(&env, &token_b);

    let to = Address::generate(&env);

    // Wire up: Router -> Factory -> Pair
    router_client.initialize(&factory_addr);
    mock_factory_client.set_pair(&token_a, &token_b, &pair_addr);
    mock_pair_client.set_reserves(&0, &0);
    mock_pair_client.set_mint_return(&1000);

    // Mint tokens to the user
    sac_a.mint(&to, &100_000);
    sac_b.mint(&to, &100_000);

    let deadline = env.ledger().timestamp() + 1000;

    (env, router_client, token_a, token_b, to, deadline, mock_pair_client, pair_addr)
}

// ── Placeholder tests (other functions still todo) ────────────────────────────

#[test]
fn test_placeholder_swap_exact_in() {
    let _env = Env::default();
}

#[test]
fn test_placeholder_swap_tokens_for_exact_tokens() {
    let _env = Env::default();
}

// ── add_liquidity tests ───────────────────────────────────────────────────────

#[test]
fn test_add_liquidity_expired_deadline() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    // Move ledger time forward so we can set a past deadline
    env.ledger().set_timestamp(2000);
    let past_deadline = env.ledger().timestamp() - 1000;

    let result = router.try_add_liquidity(
        &token_a,
        &token_b,
        &1000i128,
        &1000i128,
        &500i128,
        &500i128,
        &to,
        &past_deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::Expired)));
}

#[test]
fn test_add_liquidity_zero_amount_a() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    let result = router.try_add_liquidity(
        &token_a, &token_b, &0i128, // zero amount_a_desired
        &1000i128, &0i128, &500i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::ZeroAmount)));
}

#[test]
fn test_add_liquidity_zero_amount_b() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    let result = router.try_add_liquidity(
        &token_a, &token_b, &1000i128, &0i128, // zero amount_b_desired
        &500i128, &0i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::ZeroAmount)));
}

#[test]
fn test_add_liquidity_identical_tokens() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let to = Address::generate(&env);

    pub fn swap(_env: Env, _amount_a_out: i128, _amount_b_out: i128, _to: Address) {}

    pub fn lp_token(_env: Env) -> Address {
        panic!("not needed for router unit tests")
    }

    pub fn get_current_fee_bps(_env: Env) -> u32 {
        30
    }
}


mod helpers_test;

// Note: Full integration tests for swap functions require mock pair contracts
// These tests verify that the functions compile and basic validation works
// Full swap testing should be done in integration tests with actual pair contracts

#[test]
fn test_contract_compiles() {
    // This test ensures the router contract compiles successfully with all swap functions
    assert!(true);
}
