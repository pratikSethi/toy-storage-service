.PHONY: build test run up down up-obs down-obs docs

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
up-obs:
	TS2_OTLP_ENDPOINT=http://otel-collector:4318 docker compose -f deploy/compose/compose.yaml --profile obs up --build -d
	@echo "Grafana: http://localhost:3000 (dashboard: frontdoor)"
down-obs:
	docker compose -f deploy/compose/compose.yaml --profile obs down
docs:
	uv run mkdocs serve
