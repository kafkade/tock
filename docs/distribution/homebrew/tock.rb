# typed: false
# frozen_string_literal: true

# Homebrew formula template for tock.
#
# This file is the canonical *shape* of the formula. The `homebrew` job in
# `.github/workflows/release.yml` generates a populated copy from the real
# release artifacts (version + per-target sha256) and pushes it to
# `kafkade/homebrew-tap` as `Formula/tock.rb` on each tagged release.
#
# The URLs below intentionally match the artifact naming produced by
# `release.yml`: `tock-v#{version}-<target>.tar.gz`. The sha256 values here
# are placeholders — the release job fills in the real digests.
#
# See docs/distribution/README.md for the activation checklist.

class Tock < Formula
  desc "Unified personal productivity engine — tasks, habits, time tracking, focus timer"
  homepage "https://github.com/kafkade/tock"
  version "0.0.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "tock"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/tock --version")
  end
end
