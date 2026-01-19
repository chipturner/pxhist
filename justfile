set shell := ["zsh", "-uc"]

default:
	just --list

check:
	cargo clippy -- -D warnings
	cargo test

fmt:
	cargo fmt

cargo-upgrade *args:
	cargo-upgrade upgrade {{ args }}

coverage:
	./coverage.sh

coverage-detailed:
	./coverage-detailed.sh

coverage-clean:
	cargo llvm-cov clean --workspace
	rm -rf coverage/
	rm -f lcov.info coverage.json coverage.xml
	rm -f **/*.profraw(N) **/*.profdata(N)
