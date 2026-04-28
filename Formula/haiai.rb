class Haiai < Formula
  desc "CLI for HAI.AI agent identity, @hai.ai email, and MCP server"
  homepage "https://hai.ai"
  version "0.4.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-darwin-arm64.tar.gz"
      sha256 "d8fae11f55803793308b35282814e6e80462aa8e398d5f630b1e6aa60f1ddceb"
    end
    on_intel do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-darwin-x64.tar.gz"
      sha256 "1906267381bc3f25d9f07ab5f6afcda534710a08a249d1de4cd3dc8ece73ddc8"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-linux-arm64.tar.gz"
      sha256 "f6eafd1ce7fd5fac78d1903423db53d2159852c425c998a661772f461839538b"
    end
    on_intel do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-linux-x64.tar.gz"
      sha256 "834790f05beacc47389ef210934cb2a24ab71665978eef819273d04bb4806ac2"
    end
  end

  def install
    bin.install "haiai-cli" => "haiai"
  end

  test do
    assert_match "haiai", shell_output("#{bin}/haiai --version")
  end
end
