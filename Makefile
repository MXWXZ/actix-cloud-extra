SHELL = /bin/bash

.ONESHELL:
.PHONY: check help doc test test-features

all: help

## check: Check code and style.
check:
	@cargo clippy --all-features --all-targets -- -D clippy::all
	@cargo fmt --all -- --check

## doc: Test documents.
doc:
	@RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --no-deps --all-features --open

## test: Run tests.
test:
	@cargo test --all-features

## test-features: Test every feature individually.
test-features:
	@cargo test --no-default-features
	@for f in hyuuid utils api entity logger seaorm macros; do \
		cargo test --no-default-features --features "$$f"; \
	done

## help: Show this help.
help: Makefile
	@echo Usage: make [command]
	@sed -n 's/^##//p' $< | column -t -s ':' |  sed -e 's/^/ /'
