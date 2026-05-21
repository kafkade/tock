# Distribution channels for tock

This directory documents how `tock-cli` reaches end users. Foundation
phase: configuration is in place; not all channels are live yet.

## Channels

| Channel         | Config                                                  | Status                                                                                                                            |
|-----------------|---------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------|
| GitHub Releases | [`release.yml`](../../.github/workflows/release.yml)    | **Live** — hand-rolled matrix build for Linux/macOS/Windows.                                                                      |
| `cargo-dist`    | [`dist-workspace.toml`](../../dist-workspace.toml)      | **Config only** — validated in CI via `cargo dist plan`. Will replace `release.yml` once macOS signing + tap repo are in place.   |
| Homebrew tap    | [`homebrew/tock.rb`](homebrew/tock.rb)                  | **Template only** — needs `kafkade/homebrew-tap` repo to be created.                                                              |
| Nix flake       | [`../../flake.nix`](../../flake.nix)                    | **Dev shell live**; runnable `packages.default` deferred until tock-cli has real implementation.                                  |
| crates.io       | n/a                                                     | **Deferred** — wired in `release.yml` (commented) for a future PR.                                                                |

## macOS code signing & notarization

Signing is currently disabled. When ready:

1. Provision an Apple Developer ID Application certificate.
2. Add the following GitHub Actions secrets:
   - `APPLE_TEAM_ID`
   - `APPLE_DEVELOPER_ID_APPLICATION_P12` (base64-encoded)
   - `APPLE_DEVELOPER_ID_APPLICATION_PASSWORD`
   - `APPLE_NOTARY_USER` / `APPLE_NOTARY_TEAM_ID` / `APPLE_NOTARY_PASSWORD`
3. Flip `macos-sign = true` in `dist-workspace.toml` and re-run
   `cargo dist init`.
4. Update branch protection in `kafkade/github-infra:repo_tock.tf` if any
   workflow job names change.

## Homebrew tap

`docs/distribution/homebrew/tock.rb` is a template formula. Bringing the
tap online requires:

1. Creating the `kafkade/homebrew-tap` GitHub repo.
2. Adding a release-step in `release.yml` (or letting `cargo dist` own it)
   that pushes the formula to that tap with each tagged release.
3. Documenting `brew install kafkade/tap/tock` in the root README.
