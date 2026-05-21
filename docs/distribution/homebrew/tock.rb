# typed: false
# frozen_string_literal: true

# Homebrew formula template for tock.
#
# This file is a *template*. It is not yet wired into a tap; it lives in
# the main repo so the shape can evolve alongside `release.yml`. Once
# `kafkade/homebrew-tap` is created, automation (cargo-dist or a release
# step) will publish a populated copy of this file there on each tag.
#
# See docs/distribution/README.md for the activation checklist.

class Tock < Formula
  desc "Unified personal productivity engine — tasks, habits, time tracking, focus timer"
  homepage "https://github.com/kafkade/tock"
  version "0.0.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
    on_intel do
      url "https://github.com/kafkade/tock/releases/download/v#{version}/tock-x86_64-unknown-linux-gnu.tar.gz"
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
