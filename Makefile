.PHONY: build test run up down docs

build:
	cargo build --workspace
test:
	cargo test --workspace
run:
	cargo run -p ts2-frontdoor -- ./config/frontdoor.yaml
up:
	docker compose -f deploy/compose/compose.yaml up --build -d
down:
	docker compose -f deploy/compose/compose.yaml down
docs:
	uv run mkdocs serve
