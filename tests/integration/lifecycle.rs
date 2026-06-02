#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, Vec,
};

mod wasm {
    soroban_sdk::contractimport!(
        file = "coralswap_factory.wasm"
    );
    pub type FactoryClient<'a> = Client<'a>;
}

mod pair_wasm {
    soroban_sdk::contractimport!(
        file = "coralswap_pair.wasm"
    );
    pub type PairClient<'a> = Client<'a>;
}

mod lp_wasm {
    soroban_sdk::contractimport!(
        file = "coralswap_lp_token.wasm"
    );
    pub type LpClient<'a> = Client<'a>;
}

fn mint(_env: &Env, asset: &StellarAssetClient, to: &Address, amount: i128) {
    asset.mint(to, &amount);
}

fn deploy_factory<'a>(
    env: &'a Env,
    pair_hash: BytesN<32>,
    lp_hash: BytesN<32>,
    admin: &Address,
) -> (Address, wasm::FactoryClient<'a>) {
    let factory_addr = env.register_contract_wasm(None, wasm::WASM);
    let client = wasm::FactoryClient::new(env, &factory_addr);
    let signers = Vec::from_array(env, [admin.clone()]);
    client.try_initialize(&signers, &pair_hash, &lp_hash, admin).unwrap().unwrap();
    (factory_addr, client)
}

#[test]
fn test_factory_full_pool_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let lp_provider = Address::generate(&env);
    let swapper = Address::generate(&env);
    let token_a_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let token_b_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let (token_0_id, token_1_id) = if token_a_id < token_b_id {
        (token_a_id.clone(), token_b_id.clone())
    } else {
        (token_b_id.clone(), token_a_id.clone())
    };
    let token_0_asset = StellarAssetClient::new(&env, &token_0_id);
    let token_1_asset = StellarAssetClient::new(&env, &token_1_id);
    let token_0 = TokenClient::new(&env, &token_0_id);
    let token_1 = TokenClient::new(&env, &token_1_id);
    let pair_hash = env.deployer().upload_contract_wasm(pair_wasm::WASM);
    let lp_hash = env.deployer().upload_contract_wasm(lp_wasm::WASM);
    let (_, factory) = deploy_factory(&env, pair_hash, lp_hash, &admin);
    assert_eq!(factory.get_pair_count(), 0);
    assert!(!factory.is_paused());
    let pair_addr = factory.try_create_pair(&token_0_id, &token_1_id).unwrap().unwrap();
    assert_eq!(factory.get_pair(&token_0_id, &token_1_id), Some(pair_addr.clone()));
    assert_eq!(factory.get_pair(&token_1_id, &token_0_id), Some(pair_addr.clone()));
    assert_eq!(factory.get_pair_count(), 1);
    assert!(factory.try_create_pair(&token_0_id, &token_1_id).unwrap().is_err());
    let pair = pair_wasm::PairClient::new(&env, &pair_addr);
    let (res_a, res_b, _) = pair.try_get_reserves().unwrap().unwrap();
    assert_eq!(res_a, 0);
    assert_eq!(res_b, 0);
    let deposit_0: i128 = 1_000_000_000;
    let deposit_1: i128 = 2_000_000_000;
    mint(&env, &token_0_asset, &lp_provider, deposit_0);
    mint(&env, &token_1_asset, &lp_provider, deposit_1);
    token_0.transfer(&lp_provider, &pair_addr, &deposit_0);
    token_1.transfer(&lp_provider, &pair_addr, &deposit_1);
    let lp_minted = pair.try_mint(&lp_provider).unwrap().unwrap();
    assert!(lp_minted > 0);
    let lp_token_addr = pair.try_lp_token().unwrap().unwrap();
    let lp_token = lp_wasm::LpClient::new(&env, &lp_token_addr);
    let lp_supply_after_mint = lp_token.total_supply();
    assert_eq!(lp_token.balance(&lp_provider), lp_minted);
    assert!(lp_supply_after_mint > lp_minted);
    let (res_a, res_b, _) = pair.try_get_reserves().unwrap().unwrap();
    assert_eq!(res_a, deposit_0);
    assert_eq!(res_b, deposit_1);
    let deposit_0b: i128 = 500_000_000;
    let deposit_1b: i128 = 1_000_000_000;
    mint(&env, &token_0_asset, &lp_provider, deposit_0b);
    mint(&env, &token_1_asset, &lp_provider, deposit_1b);
    token_0.transfer(&lp_provider, &pair_addr, &deposit_0b);
    token_1.transfer(&lp_provider, &pair_addr, &deposit_1b);
    let lp_minted_2 = pair.try_mint(&lp_provider).unwrap().unwrap();
    assert!(lp_minted_2 > 0);
    assert!(lp_token.total_supply() > lp_supply_after_mint);
    let swap_in: i128 = 100_000_000;
    let (reserve_a_pre, reserve_b_pre, _) = pair.try_get_reserves().unwrap().unwrap();
    let fee_bps = pair.get_current_fee_bps() as i128;
    let in_with_fee = swap_in * (10_000 - fee_bps);
    let expected_out = (in_with_fee * reserve_b_pre) / (reserve_a_pre * 10_000 + in_with_fee);
    assert!(expected_out > 0);
    mint(&env, &token_0_asset, &swapper, swap_in);
    token_0.transfer(&swapper, &pair_addr, &swap_in);
    let bal_before = token_1.balance(&swapper);
    pair.try_swap(&0_i128, &expected_out, &swapper).unwrap().unwrap();
    assert_eq!(token_1.balance(&swapper) - bal_before, expected_out);
    let (reserve_a_post, reserve_b_post, _) = pair.try_get_reserves().unwrap().unwrap();
    assert!(reserve_a_post > reserve_a_pre);
    assert!(reserve_b_post < reserve_b_pre);
    assert!(reserve_a_post * reserve_b_post >= reserve_a_pre * reserve_b_pre);
    let swap_in_rev: i128 = 200_000_000;
    let (res_a_rev, res_b_rev, _) = pair.try_get_reserves().unwrap().unwrap();
    let fee_rev = pair.get_current_fee_bps() as i128;
    let in_fee_rev = swap_in_rev * (10_000 - fee_rev);
    let out_rev = (in_fee_rev * res_a_rev) / (res_b_rev * 10_000 + in_fee_rev);
    mint(&env, &token_1_asset, &swapper, swap_in_rev);
    token_1.transfer(&swapper, &pair_addr, &swap_in_rev);
    pair.try_swap(&out_rev, &0_i128, &swapper).unwrap().unwrap();
    let (res_a_after_rev, res_b_after_rev, _) = pair.try_get_reserves().unwrap().unwrap();
    assert!(res_b_after_rev > reserve_b_post);
    assert!(res_a_after_rev < reserve_a_post);
    let lp_bal = lp_token.balance(&lp_provider);
    assert!(lp_bal > 0);
    let (res_a_burn, res_b_burn, _) = pair.try_get_reserves().unwrap().unwrap();
    let supply_before = lp_token.total_supply();
    let exp_ret_0 = lp_bal * res_a_burn / supply_before;
    let exp_ret_1 = lp_bal * res_b_burn / supply_before;
    let tok0_before = token_0.balance(&lp_provider);
    let tok1_before = token_1.balance(&lp_provider);
    lp_token.transfer(&lp_provider, &pair_addr, &lp_bal);
    let (ret_0, ret_1) = pair.try_burn(&lp_provider).unwrap().unwrap();
    assert!(ret_0 > 0);
    assert!(ret_1 > 0);
    assert!((ret_0 - exp_ret_0).abs() <= 1);
    assert!((ret_1 - exp_ret_1).abs() <= 1);
    assert_eq!(token_0.balance(&lp_provider), tok0_before + ret_0);
    assert_eq!(token_1.balance(&lp_provider), tok1_before + ret_1);
    assert_eq!(lp_token.total_supply(), supply_before - lp_bal);
    assert_eq!(lp_token.balance(&lp_provider), 0);
    let (res_a_final, res_b_final, _) = pair.try_get_reserves().unwrap().unwrap();
    assert!(res_a_final < res_a_burn);
    assert!(res_b_final < res_b_burn);
    assert_eq!(factory.get_pair(&token_0_id, &token_1_id), Some(pair_addr.clone()));
    assert_eq!(factory.get_pair_count(), 1);
}

#[test]
fn test_create_pair_identical_tokens_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let pair_hash = env.deployer().upload_contract_wasm(pair_wasm::WASM);
    let lp_hash = env.deployer().upload_contract_wasm(lp_wasm::WASM);
    let (_, factory) = deploy_factory(&env, pair_hash, lp_hash, &admin);
    assert!(factory.try_create_pair(&token_id, &token_id).unwrap().is_err());
}

#[test]
fn test_create_pair_when_paused_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token_a_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let token_b_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let pair_hash = env.deployer().upload_contract_wasm(pair_wasm::WASM);
    let lp_hash = env.deployer().upload_contract_wasm(lp_wasm::WASM);
    let (_, factory) = deploy_factory(&env, pair_hash, lp_hash, &admin);
    let signers = Vec::from_array(&env, [admin.clone()]);
    factory.try_pause(&signers).unwrap().unwrap();
    assert!(factory.is_paused());
    assert!(factory.try_create_pair(&token_a_id, &token_b_id).unwrap().is_err());
}

#[test]
fn test_lp_supply_matches_expected_value_throughout() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let lp_provider = Address::generate(&env);
    let token_a_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let token_b_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let (token_0_id, token_1_id) = if token_a_id < token_b_id {
        (token_a_id.clone(), token_b_id.clone())
    } else {
        (token_b_id.clone(), token_a_id.clone())
    };
    let token_0_asset = StellarAssetClient::new(&env, &token_0_id);
    let token_1_asset = StellarAssetClient::new(&env, &token_1_id);
    let token_0 = TokenClient::new(&env, &token_0_id);
    let token_1 = TokenClient::new(&env, &token_1_id);
    let pair_hash = env.deployer().upload_contract_wasm(pair_wasm::WASM);
    let lp_hash = env.deployer().upload_contract_wasm(lp_wasm::WASM);
    let (_, factory) = deploy_factory(&env, pair_hash, lp_hash, &admin);
    let pair_addr = factory.try_create_pair(&token_0_id, &token_1_id).unwrap().unwrap();
    let pair = pair_wasm::PairClient::new(&env, &pair_addr);
    let deposit_0: i128 = 500_000_000;
    let deposit_1: i128 = 2_000_000_000;
    mint(&env, &token_0_asset, &lp_provider, deposit_0);
    mint(&env, &token_1_asset, &lp_provider, deposit_1);
    token_0.transfer(&lp_provider, &pair_addr, &deposit_0);
    token_1.transfer(&lp_provider, &pair_addr, &deposit_1);
    let lp_minted = pair.try_mint(&lp_provider).unwrap().unwrap();
    let lp_token_addr = pair.try_lp_token().unwrap().unwrap();
    let lp_token = lp_wasm::LpClient::new(&env, &lp_token_addr);
    let total_supply = lp_token.total_supply();
    let provider_balance = lp_token.balance(&lp_provider);
    let expected_liquidity = ((deposit_0 as f64 * deposit_1 as f64).sqrt() as i128) - 1000;
    assert!((lp_minted - expected_liquidity).abs() <= 2,
        "LP minted {lp_minted} should be close to geometric mean {expected_liquidity}");
    assert_eq!(provider_balance, lp_minted);
    assert_eq!(total_supply, lp_minted + 1000);
}
