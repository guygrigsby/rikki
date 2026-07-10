# Canonical copy lives in guygrigsby/homebrew-tap (Formula/nevla.rb); this
# is the bootstrap template. The release workflow bumps the tap's version
# line on every tag, so the two stay in sync without hands.
class Nevla < Formula
  desc "Interpreted language with Go's discipline and CPython's ecosystem"
  homepage "https://github.com/guygrigsby/nevla"
  version "0.1.0"
  # no url: the wheel comes from PyPI below, tagged for the pinned python
  depends_on "python@3.12"
  depends_on "uv" => :recommended # nevla py add drives uv

  def install
    python = Formula["python@3.12"].opt_bin/"python3.12"
    system python, "-m", "venv", libexec
    # PyPI package and both binaries share the name
    system libexec/"bin/pip", "install", "--no-cache-dir", "nevla==#{version}"
    bin.install_symlink libexec/"bin/nevla", libexec/"bin/nv"
  end

  test do
    system bin/"nevla", "--version"
    (testpath/"hi.nv").write("fn main() {\n    print(\"hi\")\n}\n")
    assert_equal "hi\n", shell_output("#{bin}/nv #{testpath}/hi.nv")
  end
end
