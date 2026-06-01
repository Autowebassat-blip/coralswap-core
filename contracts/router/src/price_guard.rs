use crate::errors::RouterError;
use soroban_sdk::{Bytes, Env};

/// Maximum age of a RedStone payload before it is considered stale (5 minutes).
pub const MAX_PAYLOAD_AGE_SECS: u64 = 300;

/// Minimal RedStone payload layout (big-endian, all fields fixed-width):
///
///   [0..16]  price      — u128, scaled by 10^8  (e.g. 100_000_000 = $1.00)
///   [16..24] timestamp  — u64 Unix seconds
///
/// Real RedStone payloads on Stellar carry a cryptographic attestation; for
/// this integration the on-chain verifier trusts the relayer to submit a
/// correctly signed payload and only enforces staleness + deviation here.
/// A production deployment would add Ed25519 signature verification over
/// bytes [0..24] using a hard-coded RedStone signer public key.
pub const PAYLOAD_LEN: u32 = 24;

/// Parses `(price_scaled, timestamp)` from a raw RedStone payload.
///
/// `price_scaled` is the oracle price multiplied by 10^8.
pub fn parse_payload(payload: &Bytes) -> Result<(u128, u64), RouterError> {
    if payload.len() != PAYLOAD_LEN {
        return Err(RouterError::InvalidOraclePayload);
    }

    // Extract price (bytes 0..16, big-endian u128)
    let mut price_bytes = [0u8; 16];
    for i in 0..16u32 {
        price_bytes[i as usize] = payload.get(i).ok_or(RouterError::InvalidOraclePayload)?;
    }
    let price = u128::from_be_bytes(price_bytes);

    // Extract timestamp (bytes 16..24, big-endian u64)
    let mut ts_bytes = [0u8; 8];
    for i in 0..8u32 {
        ts_bytes[i as usize] = payload.get(16 + i).ok_or(RouterError::InvalidOraclePayload)?;
    }
    let timestamp = u64::from_be_bytes(ts_bytes);

    if price == 0 {
        return Err(RouterError::InvalidOraclePayload);
    }

    Ok((price, timestamp))
}

/// Checks that the payload timestamp is within `MAX_PAYLOAD_AGE_SECS` of now.
pub fn check_freshness(env: &Env, payload_ts: u64) -> Result<(), RouterError> {
    let now = env.ledger().timestamp();
    let age = now.saturating_sub(payload_ts);
    if age > MAX_PAYLOAD_AGE_SECS {
        return Err(RouterError::StaleOraclePayload);
    }
    Ok(())
}

/// Checks that the execution price does not deviate from the oracle price by
/// more than `max_deviation_bps` basis points.
///
/// `exec_price_scaled` and `oracle_price_scaled` must use the same scale (10^8).
pub fn check_deviation(
    exec_price_scaled: u128,
    oracle_price_scaled: u128,
    max_deviation_bps: u32,
) -> Result<(), RouterError> {
    if oracle_price_scaled == 0 {
        return Err(RouterError::InvalidOraclePayload);
    }
    // deviation = |exec - oracle| * 10_000 / oracle  (in bps)
    let diff = if exec_price_scaled > oracle_price_scaled {
        exec_price_scaled - oracle_price_scaled
    } else {
        oracle_price_scaled - exec_price_scaled
    };
    let deviation_bps = (diff * 10_000) / oracle_price_scaled;
    if deviation_bps > max_deviation_bps as u128 {
        return Err(RouterError::PriceDeviationTooHigh);
    }
    Ok(())
}
