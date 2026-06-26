# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| V1 (current, `main` branch) | Yes |

## Reporting a Vulnerability

If you discover a security vulnerability in CoralSwap, **please report it responsibly**. Do not open a public GitHub issue.

### How to Report

1. **Email**: Send details to **security@coralswap.finance**
2. **Subject line**: `[SECURITY] CoralSwap — <brief description>`
3. **Encrypt your report** (recommended): Use the PGP key below to encrypt sensitive details.

### What to Include

- Description of the vulnerability
- Steps to reproduce or proof of concept
- Affected contracts or components (Factory, Pair, LP Token, Router)
- Potential impact assessment
- Suggested fix (if any)

### PGP Key

```
Fingerprint: (to be published by the CoralSwap security team)
```

Contact security@coralswap.finance to request the current public key.

## Response SLAs

| Severity | Acknowledgement | Triage | Resolution Target |
|---|---|---|---|
| **Critical** (fund loss, contract takeover) | 24 hours | 48 hours | 7 days |
| **High** (privilege escalation, fee manipulation) | 48 hours | 72 hours | 14 days |
| **Medium** (DoS, griefing, oracle manipulation) | 72 hours | 1 week | 30 days |
| **Low** (informational, gas optimization) | 1 week | 2 weeks | Best effort |

## Scope

The following components are in scope for security reports:

### In Scope

- **Factory contract** (`contracts/factory/`) — deployment, governance, upgrades, fee management
- **Pair contract** (`contracts/pair/`) — swaps, liquidity, flash loans, oracle, reentrancy guard
- **LP Token contract** (`contracts/lp_token/`) — minting, burning, transfers, approvals, permit
- **Router contract** (`contracts/router/`) — multi-hop routing, liquidity operations
- **Flash Receiver Interface** (`contracts/flash_receiver_interface/`) — callback interface

### Out of Scope

- Third-party token contracts
- Frontend applications and off-chain infrastructure
- Issues in dependencies managed upstream (e.g., `soroban-sdk`)
- Vulnerabilities requiring compromised Stellar validator nodes
- Social engineering attacks

## Bug Bounty

CoralSwap may offer bounty rewards for qualifying vulnerability reports at the team's discretion. Bounty amounts are determined based on severity and impact:

| Severity | Bounty Range |
|---|---|
| Critical | Negotiated case-by-case |
| High | Negotiated case-by-case |
| Medium | Negotiated case-by-case |
| Low | Recognition / credit |

### Eligibility

- First reporter of a previously unknown vulnerability
- Report must include sufficient detail to reproduce the issue
- Reporter must not exploit the vulnerability beyond what is necessary for demonstration
- Reporter must not disclose the vulnerability publicly before the agreed resolution timeline

## Known Security Measures

The CoralSwap protocol implements the following security measures:

- **Reentrancy protection**: Storage-based reentrancy guard on all state-mutating Pair operations
- **Multisig governance**: Factory pause, unpause, and upgrade operations require `ceil(n/2)` signer authorization
- **Timelocked upgrades**: 72-hour delay (~51,840 ledgers) between proposing and executing a WASM upgrade, with cancellation support
- **Double-initialization guards**: All contracts reject re-initialization
- **K invariant enforcement**: Every swap validates that the constant-product invariant holds after fee deduction
- **Overflow protection**: Arithmetic uses `checked_*` operations throughout; release builds enable `overflow-checks = true`
- **Deadline enforcement**: All user-facing Router operations accept and enforce a deadline timestamp

## Disclosure Policy

- We follow a coordinated disclosure process.
- We will work with reporters to understand and validate the issue before any public disclosure.
- Credit will be given to reporters (unless anonymity is requested) in any public advisory.
- Public disclosure will occur after a fix is deployed or after the agreed timeline, whichever comes first.
