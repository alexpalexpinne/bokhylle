.PHONY: dev-server dev-frontend install-frontend test lint frontend-build openapi openapi-check check build fixtures

dev-server:
	cargo run -p bokhylle-server

dev-frontend:
	pnpm -C frontend dev

install-frontend:
	pnpm -C frontend install

test:
	cargo test --workspace

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	pnpm -C frontend lint

frontend-build:
	pnpm -C frontend build

openapi:
	python3 scripts/check_openapi.py --write

openapi-check:
	python3 scripts/check_openapi.py

check: lint test frontend-build openapi-check

build:
	cargo build --workspace --release
	pnpm -C frontend build

fixtures:
	cargo run -p xtask -- gen-fixtures
