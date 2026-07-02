.PHONY: build release test lint install clean

build:
	cargo build

release:
	cargo build --release

# the full gate: unit + cli + goldens including the CPython bridge cases
test:
	RIKKI_TEST_PY=1 cargo test

lint:
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

# puts rikki and tk on PATH via ~/.cargo/bin
install:
	cargo install --path . --locked

clean:
	cargo clean
