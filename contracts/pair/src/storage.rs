use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub struct PairStorage {
    pub factory: Address,
    pub token_a: Address,
    pub token_b: Address,
    pub lp_token: Address,
    pub reserve_a: i128,
    pub reserve_b: i128,
    pub block_timestamp_last: u64,
    pub price_a_cumulative: i128,
    pub price_b_cumulative: i128,
    pub k_last: i128,
    /// Fee tier in basis points set at creation time (5 / 30 / 100).
    pub fee_bps: u32,
    /// Admin address authorised to pause/unpause this pair.
    pub admin: Address,
    /// Whether this pair is currently paused.
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct FeeState {
    pub vol_accumulator: i128,
    pub ema_alpha: i128,
    pub baseline_fee_bps: u32,
    pub min_fee_bps: u32,
    pub max_fee_bps: u32,
    pub ramp_up_multiplier: u32,
    pub cooldown_divisor: u32,
    pub last_fee_update: u64,
    pub decay_threshold_blocks: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct ReentrancyGuard {
    pub locked: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct OracleState {
    pub observations: soroban_sdk::Vec<(u64, i128, i128)>,
}

/// Cumulative fee and volume statistics for the pair.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FeeStatsState {
    /// Cumulative fees collected in token A (stroops) since deployment.
    pub fees_collected_0: i128,
    /// Cumulative fees collected in token B (stroops) since deployment.
    pub fees_collected_1: i128,
    /// Start timestamp (ledger seconds) of the current 24 h window.
    pub window_start: u64,
    /// Volume accumulated in the current window (sum of input amounts).
    pub volume_current: i128,
    /// Volume accumulated in the previous 24 h window (for rolling estimate).
    pub volume_previous: i128,
}

/// View-only aggregate returned by `get_fee_stats()`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FeeStats {
    /// Rolling 24 h volume estimate (current + weighted previous window).
    pub volume_24h: i128,
    /// Cumulative token-A fees collected since deployment.
    pub fees_collected_0: i128,
    /// Cumulative token-B fees collected since deployment.
    pub fees_collected_1: i128,
    /// Current dynamic fee in basis points.
    pub fee_bps: u32,
}

/// Storage keys for all persistent contract state.
#[contracttype]
pub enum DataKey {
    /// Core pair configuration and reserve state.
    PairState,
    /// Dynamic fee EMA accumulator state.
    FeeState,
    /// Reentrancy lock for flash loan guard.
    Guard,
    /// Oracle ring buffer.
    OracleState,
    /// Fee / volume statistics.
    FeeStatsState,
}

// ---------------------------------------------------------------------------
// OracleState helpers
// ---------------------------------------------------------------------------

pub fn get_oracle_state(env: &Env) -> OracleState {
    env.storage().instance().get(&DataKey::OracleState).unwrap_or(OracleState {
        observations: soroban_sdk::Vec::new(env),
    })
}

pub fn set_oracle_state(env: &Env, state: &OracleState) {
    env.storage().instance().set(&DataKey::OracleState, state);
}

// ---------------------------------------------------------------------------
// PairStorage helpers
// ---------------------------------------------------------------------------

pub fn get_pair_state(env: &Env) -> Option<PairStorage> {
    env.storage().instance().get(&DataKey::PairState)
}

pub fn set_pair_state(env: &Env, state: &PairStorage) {
    env.storage().instance().set(&DataKey::PairState, state);
}

// ---------------------------------------------------------------------------
// FeeState helpers
// ---------------------------------------------------------------------------

pub fn get_fee_state(env: &Env) -> Option<FeeState> {
    env.storage().instance().get(&DataKey::FeeState)
}

pub fn set_fee_state(env: &Env, state: &FeeState) {
    env.storage().instance().set(&DataKey::FeeState, state);
}

// ---------------------------------------------------------------------------
// Reentrancy helpers
// ---------------------------------------------------------------------------

pub fn get_reentrancy_guard(env: &Env) -> ReentrancyGuard {
    env.storage().instance().get(&DataKey::Guard).unwrap_or(ReentrancyGuard { locked: false })
}

pub fn set_reentrancy_guard(env: &Env, guard: &ReentrancyGuard) {
    env.storage().instance().set(&DataKey::Guard, guard);
}

// ---------------------------------------------------------------------------
// FeeStatsState helpers
// ---------------------------------------------------------------------------

pub fn get_fee_stats_state(env: &Env) -> FeeStatsState {
    env.storage().instance().get(&DataKey::FeeStatsState).unwrap_or(FeeStatsState {
        fees_collected_0: 0,
        fees_collected_1: 0,
        window_start: 0,
        volume_current: 0,
        volume_previous: 0,
    })
}

pub fn set_fee_stats_state(env: &Env, state: &FeeStatsState) {
    env.storage().instance().set(&DataKey::FeeStatsState, state);
}
