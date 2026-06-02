extern crate alloc;

use alloc::{vec, vec::Vec};
use core::convert::TryInto;

use redstone::{
    core::{config::Config, processor::process_payload},
    network::{error::Error as RedStoneError, StdEnv},
    Bytes as RedStoneBytes, Crypto, CryptoError, FeedId, RedStoneConfigImpl, SignerAddress,
    TimestampMillis, Value,
};
use soroban_sdk::{contracterror, crypto::Hash, Bytes, BytesN, Env, Symbol};

pub const MAX_STALE_LEDGERS: u32 = 300;

const SIGNER_THRESHOLD: u8 = 3;
const MAX_TIMESTAMP_AHEAD_MS: u64 = 60_000;
const LEDGER_CLOSE_TIME_MS: u64 = 5_000;

// RedStone primary production signer set used by the published Stellar connector.
const REDSTONE_PRIMARY_PROD_SIGNERS: [[u8; 20]; 5] = [
    hex20("8bb8f32df04c8b654987daaed53d6b6091e3b774"),
    hex20("deb22f54738d54976c4c0fe5ce6d408e40d88499"),
    hex20("51ce04be4b3e32572c4ec9135221d0691ba7d202"),
    hex20("dd682daec5a90dd295d14da4b0bec9281017b5be"),
    hex20("9c5ae89c4af6aa32ce58588dbaf90d18a855b6de"),
];

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum OracleError {
    InvalidSignature = 400,
    StalePrice = 401,
    UnsupportedAsset = 402,
    MalformedPayload = 403,
    Overflow = 404,
}

pub fn verify_and_extract_price(
    env: &Env,
    payload: Bytes,
    asset_id: Symbol,
) -> Result<(i128, u64), OracleError> {
    let feed_id = feed_id_from_symbol(env, &asset_id).ok_or(OracleError::UnsupportedAsset)?;
    let current_timestamp_ms = env.ledger().timestamp().saturating_mul(1_000);
    let max_delay_ms = u64::from(MAX_STALE_LEDGERS).saturating_mul(LEDGER_CLOSE_TIME_MS);

    let config = Config::try_new(
        SIGNER_THRESHOLD,
        REDSTONE_PRIMARY_PROD_SIGNERS
            .iter()
            .map(|signer| SignerAddress::from(signer.to_vec()))
            .collect(),
        vec![feed_id],
        TimestampMillis::from_millis(current_timestamp_ms),
        Some(TimestampMillis::from_millis(max_delay_ms)),
        Some(TimestampMillis::from_millis(MAX_TIMESTAMP_AHEAD_MS)),
    )
    .map_err(map_redstone_error)?;

    let crypto = SorobanCrypto::new(env);
    let mut redstone_config: RedStoneConfigImpl<SorobanCrypto<'_>, StdEnv> =
        (config, crypto).into();
    let validated = process_payload(&mut redstone_config, soroban_bytes_to_redstone(payload))
        .map_err(map_redstone_error)?;

    let price = validated
        .values
        .iter()
        .find(|value| value.feed == feed_id)
        .ok_or(OracleError::InvalidSignature)
        .and_then(|value| value_to_i128(value.value))?;

    Ok((price, validated.timestamp.as_millis()))
}

fn map_redstone_error(error: RedStoneError) -> OracleError {
    match error {
        RedStoneError::CryptographicError(_)
        | RedStoneError::SignerNotRecognized(_)
        | RedStoneError::InsufficientSignerCount(_, _, _) => OracleError::InvalidSignature,
        RedStoneError::TimestampTooOld(_, _)
        | RedStoneError::TimestampTooFuture(_, _)
        | RedStoneError::TimestampDifferentThanOthers(_, _) => OracleError::StalePrice,
        RedStoneError::ConfigInvalidFeedId(_) | RedStoneError::ConfigEmptyFeedIds => {
            OracleError::UnsupportedAsset
        }
        RedStoneError::NumberConversionFail | RedStoneError::NumberOverflow(_) => {
            OracleError::Overflow
        }
        _ => OracleError::MalformedPayload,
    }
}

fn feed_id_from_symbol(env: &Env, asset_id: &Symbol) -> Option<FeedId> {
    if *asset_id == Symbol::new(env, "BTC") {
        Some(FeedId::from(b"BTC".to_vec()))
    } else if *asset_id == Symbol::new(env, "ETH") {
        Some(FeedId::from(b"ETH".to_vec()))
    } else if *asset_id == Symbol::new(env, "XLM") {
        Some(FeedId::from(b"XLM".to_vec()))
    } else if *asset_id == Symbol::new(env, "USDC") {
        Some(FeedId::from(b"USDC".to_vec()))
    } else if *asset_id == Symbol::new(env, "USDT") {
        Some(FeedId::from(b"USDT".to_vec()))
    } else {
        None
    }
}

fn soroban_bytes_to_redstone(payload: Bytes) -> RedStoneBytes {
    let mut bytes = Vec::with_capacity(payload.len() as usize);
    for i in 0..payload.len() {
        bytes.push(payload.get(i).unwrap());
    }
    RedStoneBytes::from(bytes)
}

fn value_to_i128(value: Value) -> Result<i128, OracleError> {
    let bytes = value.as_be_bytes();
    if bytes[..16].iter().any(|byte| *byte != 0) {
        return Err(OracleError::Overflow);
    }

    let mut lower = [0u8; 16];
    lower.copy_from_slice(&bytes[16..]);
    i128::try_from(u128::from_be_bytes(lower)).map_err(|_| OracleError::Overflow)
}

pub struct SorobanCrypto<'a> {
    env: &'a Env,
}

impl<'a> SorobanCrypto<'a> {
    pub fn new(env: &'a Env) -> Self {
        Self { env }
    }
}

pub struct Keccak256Output {
    hash: Hash<32>,
    data: [u8; 32],
}

impl Keccak256Output {
    fn new(hash: Hash<32>) -> Self {
        let data = hash.to_array();
        Self { hash, data }
    }
}

impl AsRef<[u8]> for Keccak256Output {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Crypto for SorobanCrypto<'_> {
    type KeccakOutput = Keccak256Output;

    fn keccak256(&mut self, input: impl AsRef<[u8]>) -> Self::KeccakOutput {
        let soroban_bytes = Bytes::from_slice(self.env, input.as_ref());
        Keccak256Output::new(self.env.crypto().keccak256(&soroban_bytes))
    }

    fn recover_public_key(
        &mut self,
        recovery_byte: u8,
        signature_bytes: impl AsRef<[u8]>,
        message_hash: Self::KeccakOutput,
    ) -> Result<RedStoneBytes, CryptoError> {
        let sig_bytes = signature_bytes.as_ref();
        let sig_array: [u8; 64] =
            sig_bytes.try_into().map_err(|_| CryptoError::InvalidSignatureLen(sig_bytes.len()))?;
        let signature = BytesN::<64>::from_array(self.env, &sig_array);
        let public_key = self.env.crypto().secp256k1_recover(
            &message_hash.hash,
            &signature,
            recovery_byte.into(),
        );

        let mut bytes = vec![0u8; public_key.len() as usize];
        public_key.as_ref().copy_into_slice(&mut bytes);

        Ok(RedStoneBytes::from(bytes))
    }
}

const fn hex20(input: &str) -> [u8; 20] {
    let bytes = input.as_bytes();
    let mut out = [0u8; 20];
    let mut i = 0;
    while i < 20 {
        out[i] = (hex_val(bytes[i * 2]) << 4) | hex_val(bytes[i * 2 + 1]);
        i += 1;
    }
    out
}

const fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{verify_and_extract_price, OracleError};
    use soroban_sdk::{testutils::Ledger, Bytes, Env, Symbol};
    use std::vec::Vec;

    const BTC_PRIMARY_5SIG: &str = include_str!("testdata/BTC_PRIMARY_5sig.hex");
    const BTC_TIMESTAMP_MS: u64 = 1_744_829_680_000;
    const BTC_LEDGER_TIMESTAMP: u64 = BTC_TIMESTAMP_MS / 1_000;

    #[test]
    fn valid_known_good_payload_returns_price_and_timestamp() {
        let env = env_at(BTC_LEDGER_TIMESTAMP);

        let (price, timestamp) = verify_and_extract_price(
            &env,
            payload_bytes(&env, BTC_PRIMARY_5SIG),
            Symbol::new(&env, "BTC"),
        )
        .unwrap();

        assert_eq!(price, 8_396_206_788_771);
        assert_eq!(timestamp, BTC_TIMESTAMP_MS);
    }

    #[test]
    fn invalid_signature_returns_invalid_signature() {
        let env = env_at(BTC_LEDGER_TIMESTAMP);
        let mut payload = payload_bytes_vec(BTC_PRIMARY_5SIG);
        for signature_byte in [80usize, 222, 364] {
            payload[signature_byte] ^= 0x01;
        }

        let err = verify_and_extract_price(
            &env,
            Bytes::from_slice(&env, &payload),
            Symbol::new(&env, "BTC"),
        )
        .unwrap_err();

        assert_eq!(err, OracleError::InvalidSignature);
    }

    #[test]
    fn expired_payload_returns_stale_price() {
        let env = env_at(BTC_LEDGER_TIMESTAMP + 1_501);

        let err = verify_and_extract_price(
            &env,
            payload_bytes(&env, BTC_PRIMARY_5SIG),
            Symbol::new(&env, "BTC"),
        )
        .unwrap_err();

        assert_eq!(err, OracleError::StalePrice);
    }

    #[test]
    fn unsupported_asset_returns_unsupported_asset() {
        let env = env_at(BTC_LEDGER_TIMESTAMP);

        let err = verify_and_extract_price(
            &env,
            payload_bytes(&env, BTC_PRIMARY_5SIG),
            Symbol::new(&env, "DOGE"),
        )
        .unwrap_err();

        assert_eq!(err, OracleError::UnsupportedAsset);
    }

    fn env_at(timestamp: u64) -> Env {
        let env = Env::default();
        env.ledger().set_timestamp(timestamp);
        env
    }

    fn payload_bytes(env: &Env, hex: &str) -> Bytes {
        Bytes::from_slice(env, &payload_bytes_vec(hex))
    }

    fn payload_bytes_vec(hex: &str) -> Vec<u8> {
        let hex = hex.trim();
        assert_eq!(hex.len() % 2, 0);

        let mut bytes = Vec::with_capacity(hex.len() / 2);
        let raw = hex.as_bytes();
        let mut i = 0;
        while i < raw.len() {
            bytes.push((hex_nibble(raw[i]) << 4) | hex_nibble(raw[i + 1]));
            i += 2;
        }
        bytes
    }

    fn hex_nibble(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => panic!("invalid hex"),
        }
    }
}
