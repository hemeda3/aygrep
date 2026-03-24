class Ayg < Formula
  desc "Indexed grep - instant code search for large codebases"
  homepage "https://github.com/hemeda3/aygrep"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/hemeda3/aygrep/releases/download/v0.1.0/ayg-macos-arm64"
      sha256 "PLACEHOLDER"
    end
    on_intel do
      url "https://github.com/hemeda3/aygrep/releases/download/v0.1.0/ayg-macos-amd64"
      sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/hemeda3/aygrep/releases/download/v0.1.0/ayg-linux-arm64"
      sha256 "PLACEHOLDER"
    end
    on_intel do
      url "https://github.com/hemeda3/aygrep/releases/download/v0.1.0/ayg-linux-amd64"
      sha256 "PLACEHOLDER"
    end
  end

  def install
    binary = Dir["ayg-*"].first || "ayg"
    bin.install binary => "ayg"
  end

  test do
    mkdir "test-repo" do
      system "git", "init"
      (testpath/"test-repo/test.rs").write('fn main() { println!("hello"); }')
      system "git", "add", "."
      system "git", "commit", "-m", "init"
      system bin/"ayg", "build", "."
      output = shell_output("#{bin}/ayg search 'println' 2>&1")
      assert_match "hello", output
    end
  end
end
