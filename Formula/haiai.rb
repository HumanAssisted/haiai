class Haiai < Formula
  desc "CLI for HAI.AI agent identity, @hai.ai email, and MCP server"
  homepage "https://hai.ai"
  version "0.2.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-darwin-arm64.tar.gz"
      sha256 "7443fbd1840e85e47ede81c0629fb82410490369ddde734a455b237256145b1e"
    end
    on_intel do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-darwin-x64.tar.gz"
      sha256 "7c57c28761e23b39a56bf98f76cdec521a38b8b9877c6a90aa478751fddbfdbc"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-linux-arm64.tar.gz"
      sha256 "8b8ae9b0b536d46bbaa7126578095e566230664e34c9ec60af3b82b0e28119d3"
    end
    on_intel do
      url "https://github.com/HumanAssisted/haiai/releases/download/rust/v#{version}/haiai-cli-#{version}-linux-x64.tar.gz"
      sha256 "752aa1d261f56b53dad80f2ee3703d005fe1aa0359b44cbe85692e89b9994b4f"
    end
  end

  def install
    bin.install "haiai-cli" => "haiai"
  end

  test do
    assert_match "haiai", shell_output("#{bin}/haiai --version")
  end
end
