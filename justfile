set shell := ["zsh", "-uc"]

default:
	just --list

# Build the project
build:
	cargo build

# fmt-check + clippy (strict, incl. tests) + nextest -- mirrors the CI gate
check:
	cargo fmt -- --check
	cargo clippy --all-targets -- -D warnings
	cargo nextest run

# Recall latency guard at 500k rows (release build; ignored in `just test`)
perf:
	cargo nextest run --release --run-ignored only --test perf_test --no-capture

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
	cargo clippy --all-targets -- -D warnings
	cargo nextest run

# Run full suite N times (no retries, no fail-fast) and report pass/fail tally
stress count="10":
	#!/usr/bin/env zsh
	pass=0 fail=0
	for i in $(seq 1 {{ count }}); do
		echo -n "Run $i/{{ count }}: "
		if ! cargo nextest run --profile stress &>/dev/null; then
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

# Coverage summary (nextest, so subprocess tests count via --include-ffi)
coverage:
	cargo llvm-cov nextest --all-features --workspace --include-ffi

# Coverage with HTML report
coverage-html:
	cargo llvm-cov nextest --all-features --workspace --include-ffi --html
	@echo "Report: target/llvm-cov/html/index.html"

coverage-clean:
	cargo llvm-cov clean --workspace
	rm -rf coverage/
	rm -f lcov.info coverage.json coverage.xml
	rm -f **/*.profraw(N) **/*.profdata(N)

docker-e2e:
	docker build -t pxh-e2e -f tests/docker/Dockerfile .
	docker run --rm pxh-e2e

# Mutation testing over the pure modules in .cargo/mutants.toml (nightly CI, informational)
mutants *args:
	cargo mutants {{ args }}

# Record demo GIFs (requires vhs: https://github.com/charmbracelet/vhs)
demo *tapes:
	cargo build --release
	demo/record.sh {{ tapes }}

# Push recorded demo GIFs to gh-pages, where the README hot-links them
demo-publish:
	demo/publish.sh
