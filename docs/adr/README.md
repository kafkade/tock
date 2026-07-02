# Architecture Decision Records (ADRs)

This directory contains Architecture Decision Records for the Tock project. Each ADR documents a key architectural decision, its context, and consequences.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-001](ADR-001-rust-zero-io-core.md) | Rust with zero-I/O core crate | Accepted |
| [ADR-002](ADR-002-end-to-end-encryption.md) | End-to-end encryption with per-item envelope encryption | Accepted (amended by ADR-011) |
| [ADR-003](ADR-003-event-sourced-sync.md) | Event-sourced sync with vector clocks | Accepted |
| [ADR-004](ADR-004-sqlite-app-layer-encryption.md) | SQLite with app-layer encryption | Accepted |
| [ADR-005](ADR-005-platform-bindings.md) | Platform bindings via UniFFI and WASM | Accepted |
| [ADR-006](ADR-006-licensing-dual-license.md) | Licensing — Apache-2.0 core, AGPL-3.0 server | Accepted |
| [ADR-007](ADR-007-monetization-open-core.md) | Monetization — Open core with paid hosted sync | Accepted |
| [ADR-008](ADR-008-unified-domain-model.md) | Four unified domains — tasks, habits, time tracking, focus | Accepted |
| [ADR-009](ADR-009-natural-language-cli.md) | Natural language CLI with dual-mode parsing | Accepted |
| [ADR-010](ADR-010-srp-authentication.md) | SRP-6a authentication | Accepted (amended by ADR-011) |
| [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md) | Account-based self-host with two-secret (1Password-style) auth | Accepted (2SKD core landed in #126; format break ratified by ADR-013) |
| [ADR-012](ADR-012-client-account-onboarding.md) | Client account onboarding — Emergency Kit, Setup Code, shared orchestration | Accepted |
| [ADR-013](ADR-013-vault-format-versioning-policy.md) | Vault format versioning & 1.0 compatibility policy | Accepted |

## Categories

### Core Architecture

- ADR-001: Zero-I/O core crate design
- ADR-004: SQLite storage strategy
- ADR-005: Cross-platform bindings

### Security & Privacy

- ADR-002: End-to-end encryption design
- ADR-010: Zero-knowledge authentication
- ADR-011: Account-based self-host with two-secret (1Password-style) auth
- ADR-013: Vault format versioning & 1.0 compatibility policy

### Synchronization

- ADR-003: Event-sourced sync with conflict resolution

### Domain Model

- ADR-008: Unified task/habit/time/focus model

### User Experience

- ADR-009: Natural language CLI interface

### Business Model

- ADR-006: Open source licensing strategy
- ADR-007: Monetization model

## ADR Format

All ADRs follow this format:

```markdown
# ADR-NNN: Title

**Status:** Accepted
**Date:** YYYY-MM-DD

## Context
[What is the issue/decision?]

## Decision
[What was decided?]

## Consequences
[What are the implications?]
```

## Contributing

When proposing a new architectural decision:

1. Create a new ADR file: `ADR-NNN-short-title.md`
2. Use the next available number
3. Start with **Status:** Proposed
4. Include thorough context and consequences
5. Submit for review

Once accepted, update the status and add to this index.
