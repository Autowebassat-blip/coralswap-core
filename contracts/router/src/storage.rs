use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Factory,
    Admin,
    PriceGuardConfig,
}

/// Configuration for the swap price guard.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceGuardConfig {
    /// Minimum swap size (in token units) that triggers the oracle check.
    pub min_guarded_amount: i128,
    /// Maximum allowed deviation from oracle price, in basis points.
    pub max_deviation_bps: u32,
}

pub fn set_factory(env: &Env, factory: &Address) {
    env.storage().instance().set(&DataKey::Factory, factory);
}

pub fn get_factory(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Factory)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_price_guard_config(env: &Env, config: &PriceGuardConfig) {
    env.storage().instance().set(&DataKey::PriceGuardConfig, config);
}

pub fn get_price_guard_config(env: &Env) -> Option<PriceGuardConfig> {
    env.storage().instance().get(&DataKey::PriceGuardConfig)
}
