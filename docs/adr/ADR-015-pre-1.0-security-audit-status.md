# ADR-015: Pre-1.0 security audit status — ship 1.0 with a documented unaudited stance

**Status:** Accepted
**Date:** 2026-07-02

## Context

This ADR closes issue [#173](https://github.com/kafkade/tock/issues/173).

tock markets end-to-end encryption, a zero-knowledge sync server, SRP-6a
authentication, and two-secret key derivation (2SKD). For a **first official
release** of a privacy/crypto product, an independent third-party review of the
protocol and its implementation is effectively table stakes: it is the standard
by which users judge whether the marketed guarantees are trustworthy.

**Current assurance basis.** Today, tock's security posture rests on two things:

1. **Audited primitives.** tock uses [RustCrypto](https://github.com/RustCrypto)
   crates exclusively (`aes-gcm`, `argon2`, `hkdf`, `srp`, `x25519-dalek`,
   `ed25519-dalek`) and defines **no custom cryptographic primitives**. Those
   crates have been independently reviewed.
2. **Implementation discipline.** Domain-separated AAD, per-item key derivation,
   `Zeroize`/`ZeroizeOnDrop` on key types, redacted `Debug`, size-bucket padding,
   CI advisory checks (`cargo deny`), and the zero-I/O core boundary
   ([ADR-001](ADR-001-rust-zero-io-core.md)).

**The gap.** No external party has reviewed tock's **own** composition of those
primitives: the key hierarchy and 2SKD ([ADR-011](ADR-011-account-based-self-host-two-secret-auth.md)),
the vault format and AEAD/AAD discipline ([ADR-002](ADR-002-end-to-end-encryption.md),
[ADR-014](ADR-014-at-rest-encryption-app-layer-aead.md)), the SRP-6a handshake
plus session/token derivation and channel binding
([ADR-010](ADR-010-srp-authentication.md)), the event-sourced sync protocol and
its conflict path ([ADR-003](ADR-003-event-sourced-sync.md)), or the server's
zero-knowledge claims. Using audited building blocks does **not** imply the
assembly is correct — most real-world cryptographic failures are composition and
protocol errors, not broken primitives.

**The constraint.** A credible external audit has a long lead time (scoping,
engagement, review, remediation, publication). Audit lead-time is the long pole
for 1.0. We must decide, deliberately and on the record, how 1.0 handles this
rather than shipping in silence and letting users assume a review exists.

## Decision

**tock 1.0 ships with an explicitly documented "pre-audit / unaudited" status.**
We adopt the fallback path sanctioned by the issue's acceptance criteria: rather
than block 1.0 on an audit that cannot land in time, we make a **deliberate,
prominent, honest disclosure** and commit to commissioning the review.

Concretely:

1. **Honest disclosure, prominently placed.** `SECURITY.md` and `README.md`
   state, up front, that tock's own protocol and implementation are **not yet
   independently audited** as of 1.0 — only its underlying primitives are. The
   architecture threat model (`docs/architecture.md` §5.5) records the same
   assurance-level caveat. The disclosure explains what "unaudited" means for a
   user's threat model so they can make an informed choice.

2. **Precise framing.** We stop implying, by phrasing alone, that "audited
   RustCrypto crates" equals an audited product. The distinction —
   *primitives audited, composition not yet reviewed* — is made explicit
   everywhere the claim appears.

3. **Audit-ready scope, on the record.** We publish an auditor scoping brief
   (`docs/security/audit-scope.md`) that enumerates the surfaces an external
   review must cover, the out-of-scope items, the artifacts/pointers an auditor
   needs, and how findings will be handled. This turns "commission an audit"
   from an open-ended intention into a hand-off package: scoping is **done**;
   engaging a firm is the remaining external step.

4. **Forward commitment.** We commit to commissioning an external crypto/security
   review covering the scope above, tracking and remediating findings, and
   publishing a summary/attestation. Until that lands, the unaudited disclosure
   remains in force and is updated (not silently removed) when the status
   changes.

This is a **status and disclosure** decision. It changes **no** cryptographic
design, key hierarchy, wire format, or on-disk format. It does not weaken any
existing guarantee; it accurately states the assurance level behind those
guarantees.

## Consequences

**Positive:**

- **Honesty over silence.** Users get an explicit, informed-consent signal
  instead of an implied-but-absent audit. This is the single most important
  property for a privacy product's credibility.
- **Deliberate, documented decision.** The 1.0 assurance level is now a ratified,
  reviewable choice with a paper trail (this ADR + disclosures + scope), not an
  oversight.
- **Faster path to a real audit.** The scoping brief is the long-lead-time part
  of "commission a review." Publishing it now shortens time-to-engagement and
  gives any prospective auditor (or community reviewer) a concrete target.
- **No design risk.** Docs-only; nothing in the security-critical code paths
  changes, so there is no chance of introducing a regression while closing a
  governance gap.

**Negative:**

- **A visible "unaudited" label may deter risk-averse users** at 1.0. This is the
  correct trade-off: an accurate deterrent is preferable to a false sense of
  assurance.
- **The forward commitment is a real obligation.** Publishing intent to audit
  creates an expectation we must fund and follow through on, and keep the
  disclosure current until we do.

**Neutral:**

- The disclosure is **reversible in one direction only**: it is removed/updated
  when a genuine review lands and findings are addressed — never quietly dropped
  beforehand.
- Scope in `docs/security/audit-scope.md` is expected to evolve as the sync
  protocol and server implementation mature toward GA; it is a living document,
  versioned with the code it describes.
- This ADR sits alongside [ADR-002](ADR-002-end-to-end-encryption.md) and
  [ADR-014](ADR-014-at-rest-encryption-app-layer-aead.md) as the security-posture
  record for 1.0; it supersedes none of them.
