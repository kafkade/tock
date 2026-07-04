# Distribution channels for tock

This directory documents how `tock-cli` reaches end users. Foundation
phase: configuration is in place; not all channels are live yet.

## Channels

| Channel         | Config                                               | Status                                                                    |
|-----------------|------------------------------------------------------|---------------------------------------------------------------------------|
| GitHub Releases | [`release.yml`](../../.github/workflows/release.yml) | **Live** — cargo-dist for Linux, macOS, Windows; shell + PS installers.   |
| `cargo-dist`    | [`dist-workspace.toml`](../../dist-workspace.toml)   | **Active** — drives `release.yml` and validated in CI via `dist plan`.    |
| Homebrew tap    | [`homebrew/tock.rb`](homebrew/tock.rb)               | **Wired** — `release.yml` publishes `Formula/tock.rb` per tag; needs the tap repo + `HOMEBREW_TAP_TOKEN` (below). |
| Nix flake       | [`../../flake.nix`](../../flake.nix)                 | **Dev shell live**; packaging **deferred for 1.0** (decision below).      |
| crates.io       | [`release.yml`](../../.github/workflows/release.yml) | **Deferred for 1.0** (decision below); manual `workflow_dispatch` path exists. |

## macOS code signing & notarization

Code signing + notarization are **wired into
[`release.yml`](../../.github/workflows/release.yml)** (the `Sign and
notarize macOS binary` step in the `build` job). The step is **gated on the
signing secrets**: until they are provisioned it emits a loud warning and
ships an unsigned binary; once the secrets exist, tagged releases produce
signed + notarized macOS CLI artifacts.

> Note: the hand-written `release.yml` — not `cargo dist` — drives the
> actual build/sign/notarize today, so the `macos-sign` flag in
> `dist-workspace.toml` is intentionally left `false` (flipping it would
> imply cargo-dist owns signing, which it does not yet). Revisit when the
> release build migrates to cargo-dist.

To go live:

1. Provision an Apple Developer ID Application certificate.
2. Add the following GitHub Actions secrets:
   - `APPLE_TEAM_ID`
   - `APPLE_DEVELOPER_ID_APPLICATION_P12` (base64-encoded)
   - `APPLE_DEVELOPER_ID_APPLICATION_PASSWORD`
   - `APPLE_NOTARY_USER` / `APPLE_NOTARY_TEAM_ID` / `APPLE_NOTARY_PASSWORD`
3. Run a release dry-run (tag or `workflow_dispatch`) and confirm the step
   reports `✅ Signed and notarized` and the notarization submission
   `Accepted`.
4. No workflow **job** names change (only a step is added), so branch
   protection in `kafkade/github-infra:repo_tock.tf`
   (`required_status_checks`) needs no update. Re-verify if that ever
   changes.

A bare Mach-O CLI binary cannot be stapled; notarization registers the
binary's cdhash with Apple, and Gatekeeper validates it online on first
launch — so a downloaded binary installs without a Gatekeeper warning.

## Homebrew tap

`brew install kafkade/tap/tock` is **wired end-to-end in-repo**: the `homebrew`
job in [`release.yml`](../../.github/workflows/release.yml) runs after the
GitHub Release is created, reads the per-target `.sha256` files from the release
artifacts, renders a populated `Formula/tock.rb` (the shape mirrors the template
in [`homebrew/tock.rb`](homebrew/tock.rb)), and pushes it to the tap. It is
**gated on the `HOMEBREW_TAP_TOKEN` secret**: until that secret exists the job
emits a loud warning and skips, so releases keep working before the tap is
provisioned.

We publish from the hand-written `release.yml` rather than via cargo-dist's
`homebrew` installer, because cargo-dist does **not** drive this repo's release
(`allow-dirty = ["ci"]` in `dist-workspace.toml`) — it only validates config in
CI. Enabling cargo-dist's Homebrew publishing would be inert here and would
require regenerating the entire workflow, discarding the bespoke sign/notarize
logic. This mirrors how macOS signing is handled (see above).

### Remaining maintainer steps to go live

1. Create the `kafkade/homebrew-tap` GitHub repo (public; the `homebrew` job
   writes `Formula/tock.rb` to its default branch, `main`).
2. Provision a token with `contents: write` on that repo (a fine-grained PAT or
   a repo-scoped token) and add it to `kafkade/tock` as the
   `HOMEBREW_TAP_TOKEN` Actions secret.
3. Cut a tagged release. The `homebrew` job publishes the formula; afterwards
   `brew install kafkade/tap/tock` resolves the just-built binaries.
4. No workflow **job** names in `ci.yml` change, and `release.yml` runs on tag
   push / `workflow_dispatch` (never on PRs), so it is **not** part of branch
   protection's `required_status_checks`. No update to
   `kafkade/github-infra:repo_tock.tf` is needed. Re-verify if that ever
   changes.

## Nix packaging decision (1.0)

**Deferred for 1.0.** [`flake.nix`](../../flake.nix) ships the **dev shell
only** (`nix develop`); no `packages.default` derivation is included. Rationale:

- The first-class 1.0 install paths — GitHub Releases and the Homebrew tap —
  already cover the target audience; a Nix package is not on the critical path.
- A correct flake package for a workspace of this shape (build inputs,
  `sqlite`/`openssl` linkage, WASM feature gating) is non-trivial to get right,
  and a half-baked derivation is worse than none.

A `packages.default` derivation may land post-1.0; the dev shell remains
supported and in lockstep with `rust-toolchain.toml` in the meantime.

## crates.io decision (1.0)

**Deferred for 1.0.** The workspace has 11 crates (including the AGPL-3.0
`tock-server`), and a dependency-ordered `cargo publish` is non-trivial and
unnecessary for a **binary-first CLI release** — users install `tock` via the
channels above, not by depending on the libraries. The library APIs are also
still pre-1.0 and expected to churn.

The plumbing already exists for when we opt in: the manual `publish` job in
[`release.yml`](../../.github/workflows/release.yml)
(`workflow_dispatch` with `publish_crates: true`) publishes the non-server
crates in dependency order with dry-run validation and index waits.
`tock-server` is intentionally excluded (AGPL-3.0).
