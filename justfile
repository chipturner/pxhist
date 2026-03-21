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
	cargo update
	cargo clippy -- -D warnings
	cargo test

vendor-update:
	git submodule update --init --recursive
	cp secrets-patterns-db/db/rules-stable.yml src/vendor/rules-stable.yml
	cp src/shell_configs/bash-preexec/bash-preexec.sh src/vendor/bash-preexec.sh

coverage:
	./coverage.sh

coverage-detailed:
	./coverage-detailed.sh

coverage-clean:
	cargo llvm-cov clean --workspace
	rm -rf coverage/
	rm -f lcov.info coverage.json coverage.xml
	rm -f **/*.profraw(N) **/*.profdata(N)
