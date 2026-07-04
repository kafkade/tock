{
  description = "tock — unified personal productivity engine (tasks, habits, time, focus)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        # Read the pinned toolchain straight from rust-toolchain.toml so
        # `nix develop` and CI stay in lockstep with cargo.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        devShells.default = pkgs.mkShell {
          name = "tock-dev";
          packages = with pkgs; [
            rustToolchain
            cargo-deny
            cargo-llvm-cov
            cargo-nextest
            wasm-pack
            sqlite
            pkg-config
            openssl
          ];
          shellHook = ''
            echo "tock dev shell — Rust $(rustc --version)"
          '';
        };

        # Nix packaging is DEFERRED for 1.0 (decision recorded in
        # docs/distribution/README.md): this flake ships the dev shell only.
        # A `packages.default` derivation may land post-1.0; until then the
        # first-class install paths are GitHub Releases and the Homebrew tap.
        formatter = pkgs.nixpkgs-fmt;
      });
}
