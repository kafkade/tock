# Distribution channels for tock

This directory documents how `tock-cli` reaches end users. Foundation
phase: configuration is in place; not all channels are live yet.

## Channels

| Channel         | Config                                               | Status                                                                    |
|-----------------|------------------------------------------------------|---------------------------------------------------------------------------|
| GitHub Releases | [`release.yml`](../../.github/workflows/release.yml) | **Live** — cargo-dist for Linux, macOS, Windows; shell + PS installers.   |
| `cargo-dist`    | [`dist-workspace.toml`](../../dist-workspace.toml)   | **Active** — drives `release.yml` and validated in CI via `dist plan`.    |
| Homebrew tap    | [`homebrew/tock.rb`](homebrew/tock.rb)               | **Template only** — needs `kafkade/homebrew-tap` repo.                    |
| Nix flake       | [`../../flake.nix`](../../flake.nix)                 | **Dev shell live**; package definition deferred.                          |
| crates.io       | n/a                                                  | **Deferred** — future PR.                                                 |

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

`docs/distribution/homebrew/tock.rb` is a template formula. Bringing the
tap online requires:

1. Creating the `kafkade/homebrew-tap` GitHub repo.
2. Adding a release-step in `release.yml` (or letting `cargo dist` own it)
   that pushes the formula to that tap with each tagged release.
3. Documenting `brew install kafkade/tap/tock` in the root README.
