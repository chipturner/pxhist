set shell := ["zsh", "-uc"]

default:
	just --list

# Build the project
build:
	cargo build

# Clippy (strict) + nextest -- the pre-push gate
check:
	cargo clippy -- -D warnings
	cargo nextest run

fmt:
	cargo fmt

# Check formatting without modifying (CI-friendly)
fmt-check:
	cargo fmt -- --check

# Run tests (pass args to filter, e.g. `just test sync`)
test *args:
	cargo nextest run {{ args }}

cargo-upgrade *args:
	cargo-upgrade upgrade {{ args }}
	cargo update
	cargo clippy -- -D warnings
	cargo nextest run

# Run full suite N times and report pass/fail tally
stress count="10":
	#!/usr/bin/env zsh
	pass=0 fail=0
	for i in $(seq 1 {{ count }}); do
		echo -n "Run $i/{{ count }}: "
		if ! cargo nextest run &>/dev/null; then
			echo "FAILED"
			((fail++))
		else
			echo "PASSED"
			((pass++))
		fi
	done
	echo "\n$pass passed, $fail failed out of {{ count }} runs"
	[[ $fail -eq 0 ]]

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

docker-e2e:
	docker build -t pxh-e2e -f tests/docker/Dockerfile .
	docker run --rm pxh-e2e
