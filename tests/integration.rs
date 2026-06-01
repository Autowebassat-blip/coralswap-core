// Integration tests for the full Router swap lifecycle are located in:
//   contracts/router/src/test/mod.rs
//
// These cover:
// - swap_exact_tokens_for_tokens: deadline, zero amount, invalid path,
//   insufficient output, basic 1-hop, 2-hop, 3-hop, roundtrip
// - swap_tokens_for_exact_tokens: deadline, zero amount, invalid path,
//   excessive input, basic 1-hop, 2-hop
// - Token delivery and balance verification
//
// Run with: cargo test -p coralswap-router
