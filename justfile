set shell := ["zsh", "-uc"]

default:
	just --list

rustfmt:
	rustfmt +nightly --config edition=2021 --config imports_granularity=Crate --config group_imports=StdExternalCrate {tests,src}/**/*.rs

cargo-upgrade:
	cargo-upgrade upgrade
