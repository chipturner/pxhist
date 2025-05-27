set shell := ["zsh", "-uc"]

default:
	just --list

rustfmt:
	rustfmt --config edition=2024 --config imports_granularity=Crate --config group_imports=StdExternalCrate {tests,src}/**/*.rs

cargo-upgrade:
	cargo-upgrade upgrade

coverage:
	./coverage.sh

coverage-detailed:
	./coverage-detailed.sh

coverage-clean:
	cargo llvm-cov clean --workspace
	rm -rf coverage/
	rm -f lcov.info coverage.json coverage.xml
	rm -f **/*.profraw(N) **/*.profdata(N)
