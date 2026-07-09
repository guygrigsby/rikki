# Canonical copy lives in guygrigsby/homebrew-tap (Formula/rikki.rb); this
# is the bootstrap template. The release workflow bumps the tap's version
# line on every tag, so the two stay in sync without hands.
class Rikki < Formula
  desc "Interpreted language with Go's discipline and CPython's ecosystem"
  homepage "https://github.com/guygrigsby/rikki"
  version "0.1.0"
  # no url: the wheel comes from PyPI below, tagged for the pinned python
  depends_on "python@3.12"
  depends_on "uv" => :recommended # rikki py add drives uv

  def install
    python = Formula["python@3.12"].opt_bin/"python3.12"
    system python, "-m", "venv", libexec
    system libexec/"bin/pip", "install", "--no-cache-dir", "rikki==#{version}"
    bin.install_symlink libexec/"bin/rikki", libexec/"bin/tk"
  end

  test do
    system bin/"rikki", "--version"
    (testpath/"hi.rk").write("fn main() {\n    print(\"hi\")\n}\n")
    assert_equal "hi\n", shell_output("#{bin}/tk #{testpath}/hi.rk")
  end
end
