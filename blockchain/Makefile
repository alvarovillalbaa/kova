fmt:
	cargo fmt

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

devnet:
	docker compose -f ops/docker/docker-compose.devnet.yml up --build

