#![no_std]

pub mod malicious;

use coralswap_flash_receiver_interface::FlashReceiver;
use soroban_sdk::{
    contract, contractimpl, symbol_short, token::TokenClient, Address, Bytes, Env, Map,
};

/// Storage key for tracking loan amounts received in on_flash_loan callback
const LOAN_AMOUNT_A_KEY: &str = "loan_a";
const LOAN_AMOUNT_B_KEY: &str = "loan_b";
const LOAN_FEE_A_KEY: &str = "fee_a";
const LOAN_FEE_B_KEY: &str = "fee_b";
const CALLBACK_INVOKED_KEY: &str = "invoked";
const LOAN_INITIATOR_KEY: &str = "initiator";

#[contract]
pub struct MockFlashReceiver;

#[contractimpl]
impl FlashReceiver for MockFlashReceiver {
    fn on_flash_loan(
        env: Env,
        initiator: Address,
        token_a: Address,
        token_b: Address,
        amount_a: i128,
        amount_b: i128,
        fee_a: i128,
        fee_b: i128,
        data: Bytes,
    ) {
        // Track that callback was invoked
        env.storage().instance().set(&symbol_short!("invoked"), &true);

        // Record loan details for verification
        env.storage().instance().set(&symbol_short!("loan_a"), &amount_a);
        env.storage().instance().set(&symbol_short!("loan_b"), &amount_b);
        env.storage().instance().set(&symbol_short!("fee_a"), &fee_a);
        env.storage().instance().set(&symbol_short!("fee_b"), &fee_b);
        env.storage().instance().set(&symbol_short!("initiator"), &initiator);

        let repay_bytes = Bytes::from_slice(&env, b"repay");
        let steal_bytes = Bytes::from_slice(&env, b"steal");
        let partial_repay_bytes = Bytes::from_slice(&env, b"partial");

        if data == repay_bytes {
            // Transfer back amount + fee to the initiator (normal repayment)
            let contract_address = env.current_contract_address();

            if amount_a > 0 {
                let total_a = amount_a + fee_a;
                TokenClient::new(&env, &token_a).transfer(&contract_address, &initiator, &total_a);
            }
            if amount_b > 0 {
                let total_b = amount_b + fee_b;
                TokenClient::new(&env, &token_b).transfer(&contract_address, &initiator, &total_b);
            }
        } else if data == steal_bytes {
            // Do nothing, let the Pair invariant check fail (under-repay test)
        } else if data == partial_repay_bytes {
            // Repay only the principal without fees (this should fail)
            let contract_address = env.current_contract_address();

            if amount_a > 0 {
                TokenClient::new(&env, &token_a).transfer(&contract_address, &initiator, &amount_a);
            }
            if amount_b > 0 {
                TokenClient::new(&env, &token_b).transfer(&contract_address, &initiator, &amount_b);
            }
        }
    }
}

#[contractimpl]
impl MockFlashReceiver {
    /// Get the last recorded loan amount for token A
    pub fn get_loan_amount_a(env: Env) -> i128 {
        env.storage().instance().get(&symbol_short!("loan_a")).unwrap_or(0)
    }

    /// Get the last recorded loan amount for token B
    pub fn get_loan_amount_b(env: Env) -> i128 {
        env.storage().instance().get(&symbol_short!("loan_b")).unwrap_or(0)
    }

    /// Get the last recorded fee for token A
    pub fn get_fee_a(env: Env) -> i128 {
        env.storage().instance().get(&symbol_short!("fee_a")).unwrap_or(0)
    }

    /// Get the last recorded fee for token B
    pub fn get_fee_b(env: Env) -> i128 {
        env.storage().instance().get(&symbol_short!("fee_b")).unwrap_or(0)
    }

    /// Check if callback was invoked
    pub fn was_callback_invoked(env: Env) -> bool {
        env.storage().instance().get(&symbol_short!("invoked")).unwrap_or(false)
    }

    /// Get the recorded initiator address
    pub fn get_initiator(env: Env) -> Option<Address> {
        env.storage().instance().get(&symbol_short!("initiator"))
    }

    /// Clear all stored data (useful between tests)
    pub fn reset(env: Env) {
        env.storage().instance().remove(&symbol_short!("invoked"));
        env.storage().instance().remove(&symbol_short!("loan_a"));
        env.storage().instance().remove(&symbol_short!("loan_b"));
        env.storage().instance().remove(&symbol_short!("fee_a"));
        env.storage().instance().remove(&symbol_short!("fee_b"));
        env.storage().instance().remove(&symbol_short!("initiator"));
    }
}
