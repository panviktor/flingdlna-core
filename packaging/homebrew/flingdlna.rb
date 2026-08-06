# Homebrew formula for flingdlna
# To install from local tap: brew install --build-from-source ./flingdlna.rb
# For official tap: brew tap username/flingdlna && brew install flingdlna

class Flingdlna < Formula
  desc "DLNA/Chromecast controller and media server with TUI"
  homepage "https://github.com/username/flingdlna"
  url "https://github.com/username/flingdlna/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "PLACEHOLDER_SHA256"
  license "Apache-2.0"
  head "https://github.com/username/flingdlna.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args

    # Generate shell completions
    generate_completions_from_executable(bin/"flingdlna", "completions")
  end

  def post_install
    # Create data directory
    (var/"lib/flingdlna").mkpath
  end

  service do
    run [opt_bin/"flingdlna", "daemon", "--foreground"]
    keep_alive true
    working_dir var/"lib/flingdlna"
    log_path var/"log/flingdlna.log"
    error_log_path var/"log/flingdlna.log"
  end

  test do
    # Test help command
    assert_match "DLNA", shell_output("#{bin}/flingdlna --help")

    # Test version
    assert_match version.to_s, shell_output("#{bin}/flingdlna --version")
  end
end
