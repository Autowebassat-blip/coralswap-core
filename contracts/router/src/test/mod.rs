use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, testutils::Address as _, Address, Env,
    Vec,
};

// ---------------------------------------------------------------------------
// MockFactory
// ---------------------------------------------------------------------------

#[contract]
pub struct MockFactory;

#[contracttype]
#[derive(Clone)]
pub enum MFKey {
    Pair(Address, Address),
}

#[contractimpl]
impl MockFactory {
    pub fn set_pair(env: Env, token_a: Address, token_b: Address, pair: Address) {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().set(&MFKey::Pair(t0, t1), &pair);
    }

    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().get(&MFKey::Pair(t0, t1))
    }

    pub fn create_pair(_env: Env, _token_a: Address, _token_b: Address) -> Address {
        panic!("not needed for router unit tests")
    }
}

// ---------------------------------------------------------------------------
// MockPair
// ---------------------------------------------------------------------------

#[contract]
pub struct MockPair;

#[contracttype]
#[derive(Clone)]
pub enum MPKey {
    ReserveA,
    ReserveB,
    TokenA,
    TokenB,
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

    pub fn set_tokens(env: Env, token_a: Address, token_b: Address) {
        env.storage().instance().set(&MPKey::TokenA, &token_a);
        env.storage().instance().set(&MPKey::TokenB, &token_b);
    }

    pub fn set_burn_amounts(env: Env, amount_a: i128, amount_b: i128) {
        env.storage().instance().set(&MPKey::BurnAmountA, &amount_a);
        env.storage().instance().set(&MPKey::BurnAmountB, &amount_b);
    }

    pub fn burn(env: Env, _to: Address) -> (i128, i128) {
        let a: i128 = env.storage().instance().get(&MPKey::BurnAmountA).unwrap_or(0);
        let b: i128 = env.storage().instance().get(&MPKey::BurnAmountB).unwrap_or(0);
        (a, b)
    }

    pub fn set_liquidity_to_mint(env: Env, liquidity: i128) {
        env.storage().instance().set(&MPKey::LiquidityToMint, &liquidity);
    }

    pub fn mint(env: Env, _to: Address) -> i128 {
        env.storage().instance().get(&MPKey::LiquidityToMint).unwrap_or(0)
    }

    pub fn swap(env: Env, amount_a_out: i128, amount_b_out: i128, to: Address) {
        let token_a: Address = env.storage().instance().get(&MPKey::TokenA).unwrap();
        let token_b: Address = env.storage().instance().get(&MPKey::TokenB).unwrap();
        if amount_a_out > 0 {
            MockTokenClient::new(&env, &token_a).transfer(&env.current_contract_address(), &to, &amount_a_out);
        }
        if amount_b_out > 0 {
            MockTokenClient::new(&env, &token_b).transfer(&env.current_contract_address(), &to, &amount_b_out);
        }
        let a: i128 = env.storage().instance().get(&MPKey::ReserveA).unwrap_or(0);
        let b: i128 = env.storage().instance().get(&MPKey::ReserveB).unwrap_or(0);
        env.storage().instance().set(&MPKey::ReserveA, &(a - amount_a_out));
        env.storage().instance().set(&MPKey::ReserveB, &(b - amount_b_out));
    }

    pub fn lp_token(_env: Env) -> Address {
        panic!("not needed for router unit tests")
    }

    pub fn get_current_fee_bps(_env: Env) -> u32 {
        30
    }
}

// ---------------------------------------------------------------------------
// MockToken
// ---------------------------------------------------------------------------

#[contract]
pub struct MockToken;

#[contracttype]
#[derive(Clone)]
pub enum MTKey {
    Balance(Address),
}

#[contractimpl]
impl MockToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = MTKey::Balance(to.clone());
        let bal: i128 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(bal + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().instance().get(&MTKey::Balance(id)).unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let from_key = MTKey::Balance(from.clone());
        let from_bal: i128 = env.storage().instance().get(&from_key).unwrap_or(0);
        if from_bal < amount {
            panic!("insufficient balance");
        }
        env.storage().instance().set(&from_key, &(from_bal - amount));
        let to_key = MTKey::Balance(to.clone());
        let to_bal: i128 = env.storage().instance().get(&to_key).unwrap_or(0);
        env.storage().instance().set(&to_key, &(to_bal + amount));
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

use crate::Router;

fn deploy_router(env: &Env) -> (Address, Address) {
    let router_id = env.register_contract(None, Router);
    let factory_id = env.register_contract(None, MockFactory);
    let router = RouterClient::new(env, &router_id);
    router.initialize(&factory_id, &Vec::new(env));
    (router_id, factory_id)
}

fn generate_tokens(env: &Env, n: u32) -> Vec<Address> {
    let mut tokens: Vec<Address> = Vec::new(env);
    for _ in 0..n {
        tokens.push_back(Address::generate(env));
    }
    tokens
}

fn setup_pair(
    env: &Env,
    factory_id: &Address,
    token_a: &Address,
    token_b: &Address,
    reserve_a: i128,
    reserve_b: i128,
) -> Address {
    let pair_id = env.register_contract(None, MockPair);
    let pair = MockPairClient::new(env, &pair_id);
    pair.set_reserves(&reserve_a, &reserve_b);

    let factory = MockFactoryClient::new(env, factory_id);
    factory.set_pair(token_a, token_b, &pair_id);
    pair_id
}

fn make_path(env: &Env, tokens: &Vec<Address>) -> Vec<Address> {
    let mut path: Vec<Address> = Vec::new(env);
    for i in 0..tokens.len() {
        path.push_back(tokens.get(i).unwrap());
    }
    path
}

/// Creates a token contract and returns its address.
fn create_token(env: &Env) -> Address {
    env.register_contract(None, MockToken)
}

/// Sets up a pool with token-aware MockPair, tokens minted to the pair.
fn setup_pool(
    env: &Env,
    factory_id: &Address,
    token_a: &Address,
    token_b: &Address,
    reserve_a: i128,
    reserve_b: i128,
) -> Address {
    let pair_id = env.register_contract(None, MockPair);
    let pair = MockPairClient::new(env, &pair_id);
    pair.set_reserves(&reserve_a, &reserve_b);
    pair.set_tokens(token_a, token_b);

    // Mint tokens to the pair so it can deliver them on swap
    MockTokenClient::new(env, token_a).mint(&pair_id, &reserve_a);
    MockTokenClient::new(env, token_b).mint(&pair_id, &reserve_b);

    let factory = MockFactoryClient::new(env, factory_id);
    factory.set_pair(token_a, token_b, &pair_id);
    pair_id
}

// ---------------------------------------------------------------------------
// RouterClient helper
// ---------------------------------------------------------------------------

#[contractclient(name = "RouterClient")]
#[allow(dead_code)]
pub trait RouterInterface {
    fn initialize(env: Env, factory: Address, hubs: Vec<Address>);
    fn set_hubs(env: Env, hubs: Vec<Address>);
    fn get_hubs(env: Env) -> Vec<Address>;
    fn get_best_path(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
    ) -> (Vec<Address>, i128);
    fn swap_exact_tokens_multi_hop(
        env: Env,
        path: Vec<Address>,
        amount_in: i128,
        amount_out_min: i128,
        to: Address,
        deadline: u64,
    ) -> i128;
    fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Vec<i128>;
    fn swap_tokens_for_exact_tokens(
        env: Env,
        amount_out: i128,
        amount_in_max: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Vec<i128>;
    fn add_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> (i128, i128, i128);
    fn remove_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        liquidity: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> (i128, i128);
}

mod helpers_test;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_contract_compiles() {
    // Contract compiles and links correctly
}

// ===================== get_best_path =====================

#[test]
fn test_get_best_path_identical_tokens() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let token = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&token, &token, &1000);
    }));
    assert!(result.is_err(), "identical tokens must fail");
}

#[test]
fn test_get_best_path_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&a, &b, &0);
    }));
    assert!(result.is_err(), "zero amount must fail");
}

#[test]
fn test_get_best_path_no_factory_set() {
    let env = Env::default();
    let router_id = env.register_contract(None, Router);
    let router = RouterClient::new(&env, &router_id);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&a, &b, &1000);
    }));
    assert!(result.is_err(), "no factory must fail");
}

#[test]
fn test_get_best_path_direct_pair() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();

    setup_pair(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 2, "direct path must have 2 entries");
    assert_eq!(path.get(0).unwrap(), token_a);
    assert_eq!(path.get(1).unwrap(), token_b);
    assert!(expected_out > 0, "expected output must be positive");
}

#[test]
fn test_get_best_path_two_hop() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 3);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let hub = tokens.get(2).unwrap();

    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(hub.clone());
    router.set_hubs(&hubs);

    setup_pair(&env, &factory_id, &token_a, &hub, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub, &token_b, 200_000, 200_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 3, "2-hop path must have 3 entries");
    assert_eq!(path.get(0).unwrap(), token_a);
    assert_eq!(path.get(1).unwrap(), hub);
    assert_eq!(path.get(2).unwrap(), token_b);
    assert!(expected_out > 0, "expected output must be positive");
}

#[test]
fn test_get_best_path_prefers_highest_output() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 4);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let hub1 = tokens.get(2).unwrap();
    let hub2 = tokens.get(3).unwrap();

    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(hub1.clone());
    hubs.push_back(hub2.clone());
    router.set_hubs(&hubs);

    // Direct pair with very low liquidity → low output
    setup_pair(&env, &factory_id, &token_a, &token_b, 500, 500);

    // hub1 route with high liquidity
    setup_pair(&env, &factory_id, &token_a, &hub1, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub1, &token_b, 100_000, 100_000);

    // hub2 route with low liquidity
    setup_pair(&env, &factory_id, &token_a, &hub2, 1_000, 1_000);
    setup_pair(&env, &factory_id, &hub2, &token_b, 1_000, 1_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 3, "should select 2-hop via best hub");
    assert_eq!(path.get(1).unwrap(), hub1, "should prefer higher-liquidity hub");
    assert!(expected_out > 0);
}

#[test]
fn test_get_best_path_three_hop() {
    let env = Env::default();
    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 4);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();
    let hub1 = tokens.get(2).unwrap();
    let hub2 = tokens.get(3).unwrap();

    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(hub1.clone());
    hubs.push_back(hub2.clone());
    router.set_hubs(&hubs);

    setup_pair(&env, &factory_id, &token_a, &hub1, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub1, &hub2, 100_000, 100_000);
    setup_pair(&env, &factory_id, &hub2, &token_b, 100_000, 100_000);

    let (path, expected_out) = router.get_best_path(&token_a, &token_b, &1000);
    assert_eq!(path.len(), 4, "3-hop path must have 4 entries");
    assert_eq!(path.get(0).unwrap(), token_a);
    assert_eq!(path.get(1).unwrap(), hub1);
    assert_eq!(path.get(2).unwrap(), hub2);
    assert_eq!(path.get(3).unwrap(), token_b);
    assert!(expected_out > 0);
}

#[test]
fn test_get_best_path_no_route() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let token_a = tokens.get(0).unwrap();
    let token_b = tokens.get(1).unwrap();

    // Set up a hub but no pairs connecting token_a or token_b
    let mut hubs: Vec<Address> = Vec::new(&env);
    hubs.push_back(Address::generate(&env));
    router.set_hubs(&hubs);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.get_best_path(&token_a, &token_b, &1000);
    }));
    assert!(result.is_err(), "no feasible route must fail");
}

// ===================== swap_exact_tokens_multi_hop =====================

#[test]
fn test_swap_multi_hop_expired_deadline() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(
            &path,
            &1000,
            &1,
            &Address::generate(&env),
            &1, // deadline in the past (ledger timestamp is 2000)
        );
    }));
    assert!(result.is_err(), "expired deadline must fail");
}

#[test]
fn test_swap_multi_hop_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(&path, &0, &1, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "zero amount must fail");
}

#[test]
fn test_swap_multi_hop_invalid_path_too_short() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(&path, &1000, &1, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "too-short path must fail");
}

#[test]
fn test_swap_multi_hop_invalid_path_too_long() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 5);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_multi_hop(&path, &1000, &1, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "too-long path (4+ hops) must fail");
}

// ===================== swap_tokens_for_exact_tokens =====================

#[test]
fn test_swap_exact_out_expired_deadline() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(&100, &1000, &path, &Address::generate(&env), &1);
    }));
    assert!(result.is_err(), "expired deadline must fail");
}

#[test]
fn test_swap_exact_out_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(&0, &1000, &path, &Address::generate(&env), &u64::MAX);
    }));
    assert!(result.is_err(), "zero output amount must fail");
}

#[test]
fn test_swap_exact_out_invalid_path() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(
            &100,
            &1000,
            &path,
            &Address::generate(&env),
            &u64::MAX,
        );
    }));
    assert!(result.is_err(), "too-short path must fail");
}

// ===================== swap_exact_tokens_for_tokens lifecycle =====================

#[test]
fn test_swap_exact_tokens_for_tokens_expired_deadline() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_for_tokens(
            &1000,
            &1,
            &path,
            &Address::generate(&env),
            &1,
        );
    }));
    assert!(result.is_err(), "expired deadline must fail");
}

#[test]
fn test_swap_exact_tokens_for_tokens_zero_amount() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let tokens = generate_tokens(&env, 2);
    let path = make_path(&env, &tokens);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_for_tokens(
            &0,
            &1,
            &path,
            &Address::generate(&env),
            &u64::MAX,
        );
    }));
    assert!(result.is_err(), "zero amount must fail");
}

#[test]
fn test_swap_exact_tokens_for_tokens_invalid_path() {
    let env = Env::default();
    let (router_id, _factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);
    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(Address::generate(&env));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_for_tokens(
            &1000,
            &1,
            &path,
            &Address::generate(&env),
            &u64::MAX,
        );
    }));
    assert!(result.is_err(), "too-short path must fail");
}

#[test]
fn test_swap_exact_tokens_for_tokens_basic() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let user = Address::generate(&env);

    let reserve_a = 100_000i128;
    let reserve_b = 100_000i128;
    setup_pool(&env, &factory_id, &token_a, &token_b, reserve_a, reserve_b);

    let amount_in = 1000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &amount_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());

    let amounts = router.swap_exact_tokens_for_tokens(
        &amount_in,
        &1,
        &path,
        &user,
        &u64::MAX,
    );

    assert_eq!(amounts.len(), 1, "1-hop path returns 1 amount");
    let amount_out = amounts.get(0).unwrap();
    assert!(amount_out > 0, "output amount must be positive");
    assert!(amount_out < amount_in, "output must be less than input due to fee");

    let user_balance_b = MockTokenClient::new(&env, &token_b).balance(&user);
    assert_eq!(user_balance_b, amount_out, "user must receive output tokens");

    let user_balance_a = MockTokenClient::new(&env, &token_a).balance(&user);
    assert_eq!(user_balance_a, 0, "user input must be fully spent");
}

#[test]
fn test_swap_exact_tokens_for_tokens_insufficient_output() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let user = Address::generate(&env);

    setup_pool(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);

    let amount_in = 1000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &amount_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_exact_tokens_for_tokens(
            &amount_in,
            &u64::MAX as i128,
            &path,
            &user,
            &u64::MAX,
        );
    }));
    assert!(result.is_err(), "insufficient output must fail");
}

#[test]
fn test_swap_exact_tokens_for_tokens_two_hop() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let token_c = create_token(&env);
    let user = Address::generate(&env);

    setup_pool(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);
    setup_pool(&env, &factory_id, &token_b, &token_c, 100_000, 100_000);

    let amount_in = 1000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &amount_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());
    path.push_back(token_c.clone());

    let amounts = router.swap_exact_tokens_for_tokens(
        &amount_in,
        &1,
        &path,
        &user,
        &u64::MAX,
    );

    assert_eq!(amounts.len(), 2, "2-hop path returns 2 amounts");
    let mid_out = amounts.get(0).unwrap();
    let final_out = amounts.get(1).unwrap();
    assert!(mid_out > 0, "intermediate output must be positive");
    assert!(final_out > 0, "final output must be positive");
    assert!(final_out < mid_out, "second hop reduces output further");

    let user_balance_c = MockTokenClient::new(&env, &token_c).balance(&user);
    assert_eq!(user_balance_c, final_out, "user must receive final output tokens");

    let user_balance_a = MockTokenClient::new(&env, &token_a).balance(&user);
    assert_eq!(user_balance_a, 0, "user input must be fully spent");
}

#[test]
fn test_swap_exact_tokens_for_tokens_three_hop() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let token_c = create_token(&env);
    let token_d = create_token(&env);
    let user = Address::generate(&env);

    setup_pool(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);
    setup_pool(&env, &factory_id, &token_b, &token_c, 100_000, 100_000);
    setup_pool(&env, &factory_id, &token_c, &token_d, 100_000, 100_000);

    let amount_in = 10000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &amount_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());
    path.push_back(token_c.clone());
    path.push_back(token_d.clone());

    let amounts = router.swap_exact_tokens_for_tokens(
        &amount_in,
        &1,
        &path,
        &user,
        &u64::MAX,
    );

    assert_eq!(amounts.len(), 3, "3-hop path returns 3 amounts");
    let final_out = amounts.get(2).unwrap();
    assert!(final_out > 0, "final output must be positive");

    let user_balance_d = MockTokenClient::new(&env, &token_d).balance(&user);
    assert_eq!(user_balance_d, final_out, "user must receive final output tokens");
}

// ===================== swap_tokens_for_exact_tokens lifecycle =====================

#[test]
fn test_swap_exact_out_excessive_input() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let user = Address::generate(&env);

    setup_pool(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);

    let amount_out = 900i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &10_000_000);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());

    // Set max_input to a very small value to force excessive input error
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        router.swap_tokens_for_exact_tokens(
            &amount_out,
            &1,
            &path,
            &user,
            &u64::MAX,
        );
    }));
    assert!(result.is_err(), "excessive input must fail when max_input is too low");
}

#[test]
fn test_swap_tokens_for_exact_tokens_basic() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let user = Address::generate(&env);

    let reserve_a = 100_000i128;
    let reserve_b = 100_000i128;
    setup_pool(&env, &factory_id, &token_a, &token_b, reserve_a, reserve_b);

    let amount_out = 900i128;
    let max_in = 2000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &max_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());

    let amounts = router.swap_tokens_for_exact_tokens(
        &amount_out,
        &max_in,
        &path,
        &user,
        &u64::MAX,
    );

    assert_eq!(amounts.len(), 1, "1-hop path returns 1 amount for exact output");
    let amount_in_used = amounts.get(0).unwrap();
    assert!(amount_in_used > 0, "input amount must be positive");
    assert!(amount_in_used <= max_in, "input must not exceed max");

    let user_balance_b = MockTokenClient::new(&env, &token_b).balance(&user);
    assert_eq!(user_balance_b, amount_out, "user must receive exact output amount");

    let user_balance_a = MockTokenClient::new(&env, &token_a).balance(&user);
    assert_eq!(user_balance_a, max_in - amount_in_used, "remaining input must be correct");
}

#[test]
fn test_swap_tokens_for_exact_tokens_two_hop() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let token_c = create_token(&env);
    let user = Address::generate(&env);

    setup_pool(&env, &factory_id, &token_a, &token_b, 100_000, 100_000);
    setup_pool(&env, &factory_id, &token_b, &token_c, 100_000, 100_000);

    let amount_out = 900i128;
    let max_in = 5000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &max_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());
    path.push_back(token_c.clone());

    let amounts = router.swap_tokens_for_exact_tokens(
        &amount_out,
        &max_in,
        &path,
        &user,
        &u64::MAX,
    );

    assert_eq!(amounts.len(), 2, "2-hop path returns 2 amounts");
    let amount_in_used = amounts.get(0).unwrap();
    assert!(amount_in_used > 0, "first-hop input must be positive");
    assert!(amount_in_used <= max_in, "total input must not exceed max");

    let user_balance_c = MockTokenClient::new(&env, &token_c).balance(&user);
    assert_eq!(user_balance_c, amount_out, "user must receive exact output amount");
}

#[test]
fn test_swap_both_directions_roundtrip() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let (router_id, factory_id) = deploy_router(&env);
    let router = RouterClient::new(&env, &router_id);

    let token_a = create_token(&env);
    let token_b = create_token(&env);
    let user = Address::generate(&env);

    setup_pool(&env, &factory_id, &token_a, &token_b, 1_000_000, 1_000_000);

    let amount_in = 10_000i128;
    MockTokenClient::new(&env, &token_a).mint(&user, &amount_in);

    let mut path: Vec<Address> = Vec::new(&env);
    path.push_back(token_a.clone());
    path.push_back(token_b.clone());

    // Swap A -> B
    let amounts_out = router.swap_exact_tokens_for_tokens(
        &amount_in,
        &1,
        &path,
        &user,
        &u64::MAX,
    );
    let token_b_received = amounts_out.get(amounts_out.len() - 1).unwrap();
    assert!(token_b_received > 0, "must receive B tokens");

    // Now swap back B -> A
    let mut reverse_path: Vec<Address> = Vec::new(&env);
    reverse_path.push_back(token_b.clone());
    reverse_path.push_back(token_a.clone());

    let amounts_back = router.swap_exact_tokens_for_tokens(
        &token_b_received,
        &1,
        &reverse_path,
        &user,
        &u64::MAX,
    );
    let token_a_recovered = amounts_back.get(amounts_back.len() - 1).unwrap();
    assert!(token_a_recovered > 0, "must recover some A tokens");
    assert!(token_a_recovered < amount_in, "roundtrip loses value due to fees");
}
