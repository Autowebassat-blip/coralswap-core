//! Tests for Router::swap_with_price_guard and Router::set_price_guard_config.

use crate::{Router, RouterClient, RouterError};
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger as _},
    token::StellarAssetClient,
    Address, Bytes, Env, Vec,
};

// ── Minimal mocks (factory + pair) ────────────────────────────────────────────

#[contract]
pub struct PGFactory;

#[contracttype]
#[derive(Clone)]
pub enum PGFKey {
    Pair(Address, Address),
}

#[contractimpl]
impl PGFactory {
    pub fn set_pair(env: Env, token_a: Address, token_b: Address, pair: Address) {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().set(&PGFKey::Pair(t0, t1), &pair);
    }
    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().get(&PGFKey::Pair(t0, t1))
    }
    pub fn create_pair(env: Env, token_a: Address, token_b: Address) -> Address {
        let (t0, t1) = if token_a < token_b { (token_a, token_b) } else { (token_b, token_a) };
        env.storage().instance().get(&PGFKey::Pair(t0, t1)).unwrap()
    }
}

#[contract]
pub struct PGPair;

#[contracttype]
#[derive(Clone)]
pub enum PGPKey {
    ReserveA,
    ReserveB,
    FeeBps,
}

#[contractimpl]
impl PGPair {
    pub fn set_reserves(env: Env, ra: i128, rb: i128) {
        env.storage().instance().set(&PGPKey::ReserveA, &ra);
        env.storage().instance().set(&PGPKey::ReserveB, &rb);
    }
    pub fn set_fee_bps(env: Env, fee: u32) {
        env.storage().instance().set(&PGPKey::FeeBps, &fee);
    }
    pub fn get_reserves(env: Env) -> (i128, i128, u64) {
        let ra: i128 = env.storage().instance().get(&PGPKey::ReserveA).unwrap_or(0);
        let rb: i128 = env.storage().instance().get(&PGPKey::ReserveB).unwrap_or(0);
        (ra, rb, 0u64)
    }
    pub fn get_current_fee_bps(env: Env) -> u32 {
        env.storage().instance().get(&PGPKey::FeeBps).unwrap_or(30)
    }
    pub fn swap(_env: Env, _a_out: i128, _b_out: i128, _to: Address) {}
    // Unused by swap_with_price_guard but required by PairInterface
    pub fn mint(_env: Env, _to: Address) -> i128 { 0 }
    pub fn burn(_env: Env, _to: Address) -> (i128, i128) { (0, 0) }
    pub fn lp_token(_env: Env) -> Address { panic!("not used") }
}

// ── Helper: build a valid 24-byte RedStone payload ────────────────────────────

/// Encodes `(price_scaled: u128, timestamp: u64)` into 24 big-endian bytes.
fn make_payload(env: &Env, price_scaled: u128, timestamp: u64) -> Bytes {
    let mut buf = [0u8; 24];
    buf[0..16].copy_from_slice(&price_scaled.to_be_bytes());
    buf[16..24].copy_from_slice(&timestamp.to_be_bytes());
    Bytes::from_slice(env, &buf)
}

// ── Test setup ────────────────────────────────────────────────────────────────

struct Setup {
    env: Env,
    router: RouterClient<'static>,
    token_in: Address,
    token_out: Address,
    user: Address,
    #[allow(dead_code)]
    pair: Address,
    #[allow(dead_code)]
    admin: Address,
}

impl Setup {
    fn new(reserve_in: i128, reserve_out: i128) -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000_000);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        let factory_addr = env.register_contract(None, PGFactory);
        let factory_client = PGFactoryClient::new(&env, &factory_addr);

        let pair_addr = env.register_contract(None, PGPair);
        let pair_client = PGPairClient::new(&env, &pair_addr);
        pair_client.set_reserves(&reserve_in, &reserve_out);
        pair_client.set_fee_bps(&30);

        let token_admin = Address::generate(&env);
        let token_in = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_out = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

        // Ensure token_in < token_out so reserves map correctly
        let (token_in, token_out) = if token_in < token_out {
            (token_in, token_out)
        } else {
            (token_out, token_in)
        };

        factory_client.set_pair(&token_in, &token_out, &pair_addr);
        StellarAssetClient::new(&env, &token_in).mint(&user, &1_000_000_000);

        let router_addr = env.register_contract(None, Router);
        let router = RouterClient::new(&env, &router_addr);
        router.initialize_with_admin(&factory_addr, &admin);

        Setup { env, router, token_in, token_out, user, pair: pair_addr, admin }
    }

    fn set_guard(&self, min_amount: i128, max_deviation_bps: u32) {
        self.router.set_price_guard_config(&min_amount, &max_deviation_bps);
    }

    fn path(&self) -> Vec<Address> {
        let mut v = Vec::new(&self.env);
        v.push_back(self.token_in.clone());
        v.push_back(self.token_out.clone());
        v
    }

    fn deadline(&self) -> u64 {
        self.env.ledger().timestamp() + 3600
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Small swap below the guard threshold executes without any payload.
#[test]
fn test_below_threshold_no_payload_succeeds() {
    let s = Setup::new(10_000_000, 10_000_000);
    // Guard: only trigger for swaps >= 1_000_000; we send 100
    s.set_guard(1_000_000, 200);

    let result = s.router.try_swap_with_price_guard(
        &100_i128,
        &1_i128,
        &s.path(),
        &s.user,
        &s.deadline(),
        &None,
    );
    assert!(result.is_ok(), "small swap should bypass guard: {:?}", result);
}

/// Valid payload within 5 minutes and within deviation threshold — swap executes.
#[test]
fn test_valid_payload_swap_executes() {
    // reserves 1:1, so exec_price ≈ 1.0 (scaled = 100_000_000)
    let s = Setup::new(10_000_000, 10_000_000);
    s.set_guard(1_000, 200); // guard on, 2% max deviation

    let now = s.env.ledger().timestamp();
    // oracle price = 1.0 scaled, fresh timestamp
    let payload = make_payload(&s.env, 100_000_000, now);

    let result = s.router.try_swap_with_price_guard(
        &10_000_i128,
        &1_i128,
        &s.path(),
        &s.user,
        &s.deadline(),
        &Some(payload),
    );
    assert!(result.is_ok(), "valid payload should pass guard: {:?}", result);
}

/// Payload older than 5 minutes reverts with StaleOraclePayload.
#[test]
fn test_stale_payload_reverts() {
    let s = Setup::new(10_000_000, 10_000_000);
    s.set_guard(1_000, 200);

    let now = s.env.ledger().timestamp();
    let stale_ts = now - 301; // 5 min 1 sec old
    let payload = make_payload(&s.env, 100_000_000, stale_ts);

    let result = s.router.try_swap_with_price_guard(
        &10_000_i128,
        &1_i128,
        &s.path(),
        &s.user,
        &s.deadline(),
        &Some(payload),
    );
    assert_eq!(result, Err(Ok(RouterError::StaleOraclePayload)));
}

/// Execution price deviates more than max_deviation_bps from oracle price.
#[test]
fn test_deviation_too_high_reverts() {
    // reserves 1:1 → exec_price_scaled ≈ 100_000_000 (1.0)
    let s = Setup::new(10_000_000, 10_000_000);
    s.set_guard(1_000, 100); // 1% max deviation

    let now = s.env.ledger().timestamp();
    // Oracle says price is 1.05 (5% above exec price) → deviation = 5% > 1%
    let oracle_price = 105_000_000_u128;
    let payload = make_payload(&s.env, oracle_price, now);

    let result = s.router.try_swap_with_price_guard(
        &10_000_i128,
        &1_i128,
        &s.path(),
        &s.user,
        &s.deadline(),
        &Some(payload),
    );
    assert_eq!(result, Err(Ok(RouterError::PriceDeviationTooHigh)));
}

/// Payload exactly at the staleness boundary (300 s old) is still accepted.
#[test]
fn test_payload_at_staleness_boundary_passes() {
    let s = Setup::new(10_000_000, 10_000_000);
    s.set_guard(1_000, 200);

    let now = s.env.ledger().timestamp();
    let boundary_ts = now - 300; // exactly 5 minutes
    let payload = make_payload(&s.env, 100_000_000, boundary_ts);

    let result = s.router.try_swap_with_price_guard(
        &10_000_i128,
        &1_i128,
        &s.path(),
        &s.user,
        &s.deadline(),
        &Some(payload),
    );
    assert!(result.is_ok(), "payload at exact boundary should pass: {:?}", result);
}

/// set_price_guard_config rejects callers that are not the admin.
#[test]
fn test_set_price_guard_config_unauthorized() {
    let env = Env::default();
    // Do NOT mock_all_auths — let auth checks run normally
    let router_addr = env.register_contract(None, Router);
    let router = RouterClient::new(&env, &router_addr);
    let factory = Address::generate(&env);
    router.initialize(&factory);
    // No admin set → Unauthorized
    let result = router.try_set_price_guard_config(&1_000_i128, &200_u32);
    assert_eq!(result, Err(Ok(RouterError::Unauthorized)));
}
