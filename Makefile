.PHONY: build test run up down up-obs down-obs docs cli e2e

build:
	cargo build --workspace
test:
	cargo test --workspace
run:
	cargo run -p ts2-frontdoor -- ./config/frontdoor.yaml
up:
	docker compose -f deploy/compose/compose.yaml up --build -d
	@echo "FrontdoorService: http://127.0.0.1:9001"
down:
	docker compose -f deploy/compose/compose.yaml down
up-obs:
	TS2_OTLP_ENDPOINT=http://otel-collector:4318 docker compose -f deploy/compose/compose.yaml --profile obs up --build -d
	@echo "FrontdoorService: http://127.0.0.1:9001"
	@echo "Grafana: http://localhost:3000 (dashboard: frontdoor)"
down-obs:
	docker compose -f deploy/compose/compose.yaml --profile obs down
docs:
	uv run mkdocs serve
cli:
	uv sync --project cli
# Starts a throwaway frontdoor on :9099 with its own data dir, runs the smoke test, stops it.
E2E_DIR := $(CURDIR)/target/e2e
e2e: cli
	cargo build -p ts2-frontdoor
	rm -rf $(E2E_DIR) && mkdir -p $(E2E_DIR)
	sed -e 's|127.0.0.1:9000|127.0.0.1:9099|' -e 's|\./data|$(E2E_DIR)/data|' config/frontdoor.yaml > $(E2E_DIR)/frontdoor.yaml
	@./target/debug/ts2-frontdoor $(E2E_DIR)/frontdoor.yaml > $(E2E_DIR)/frontdoor.log 2>&1 & echo $$! > $(E2E_DIR)/pid; \
	trap 'kill $$(cat $(E2E_DIR)/pid)' EXIT; \
	for i in $$(seq 50); do curl -sf http://127.0.0.1:9099/healthz >/dev/null && break; sleep 0.2; done; \
	TS2_ENDPOINT=http://127.0.0.1:9099 uv run --project cli pytest tests/e2e -v
