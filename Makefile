COMPOSE := docker compose -f deploy/compose/compose.yaml

.PHONY: images build test run up down up-obs down-obs docs cli e2e e2e-native e2e-compose

build:
	cargo build --workspace
test:
	cargo test --workspace
run:
	cargo run -p ts2-frontdoor -- ./config/frontdoor.yaml
# One at a time: a cold chunk-store build compiles gRPC, and the other C++ image reuses its layer (see docs/setup.md).
images:
	$(COMPOSE) build chunk-store-1
	$(COMPOSE) build object-index
	$(COMPOSE) build frontdoor
up: images
	$(COMPOSE) up -d
	@echo "FrontdoorService: http://127.0.0.1:9001"
down:
	docker compose -f deploy/compose/compose.yaml down
up-obs: images
	TS2_OTLP_ENDPOINT=http://otel-collector:4318 $(COMPOSE) --profile obs up -d
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

# Same smoke test with the frontdoor in remote mode against natively built C++ services (cmake debug builds
# in chunk-store/ and object-index/) on throwaway ports 50151 and 50152 and temp data dirs.
CPP_DIR := $(CURDIR)/target/e2e-cpp
e2e-native:
	@test -x chunk-store/build/debug/ts2-chunk-store -a -x object-index/build/debug/ts2-object-index || { echo "build the C++ services first (see chunk-store/README.md)"; exit 1; }
	rm -rf $(CPP_DIR) && mkdir -p $(CPP_DIR)
	@TS2_LISTEN=127.0.0.1:50151 TS2_DATA_DIR=$(CPP_DIR)/chunks chunk-store/build/debug/ts2-chunk-store > $(CPP_DIR)/chunk-store.log 2>&1 & echo $$! > $(CPP_DIR)/pids; \
	TS2_LISTEN=127.0.0.1:50152 TS2_DATA_DIR=$(CPP_DIR)/index object-index/build/debug/ts2-object-index > $(CPP_DIR)/object-index.log 2>&1 & echo $$! >> $(CPP_DIR)/pids; \
	trap 'kill $$(cat $(CPP_DIR)/pids)' EXIT; sleep 1; \
	$(MAKE) e2e TS2_INDEX_ADDR=127.0.0.1:50152 TS2_CHUNK_NODES=node-1=127.0.0.1:50151 \
	  TS2_RESTART_CHUNK_STORE='tests/e2e/restart_chunk_store_native.sh $(CPP_DIR)'

# The smoke test against the compose stack from `make up` (FrontdoorService on 9001). Leaves the stack running.
e2e-compose: cli
	TS2_ENDPOINT=http://127.0.0.1:9001 TS2_RESTART_CHUNK_STORE='docker compose -f deploy/compose/compose.yaml restart chunk-store-1' \
	  uv run --project cli pytest tests/e2e -v
