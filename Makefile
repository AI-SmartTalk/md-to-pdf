dcrust=$$( [ -f /.dockerenv ] && echo "" || echo "docker compose exec rust")
dcpandoc=$$( [ -f /.dockerenv ] && echo "" || echo "docker compose exec pandoc")

.PHONY: it
it: fmt target/debug ## Perform common targets

.PHONY: help
help: ## Displays this list of targets with descriptions
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(firstword $(MAKEFILE_LIST)) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[32m%-30s\033[0m %s\n", $$1, $$2}'

.PHONY: dev
dev: dc-build up target/debug serve ## Build, compile and serve in dev mode (hot reload)

.PHONY: prod
prod: ## Build and run the production image
	docker compose -f docker-compose.prod.yml build --pull
	docker compose -f docker-compose.prod.yml up -d

.PHONY: prod-down
prod-down: ## Stop the production container
	docker compose -f docker-compose.prod.yml down

.PHONY: setup
setup: dc-build cargo-deps ## Set up the local environment

.PHONY: dc-build
dc-build: ## Build the local dev image
	docker compose build --pull

.PHONY: up
up: ## Bring up the containers
	[ -f /.dockerenv ] || docker compose up --detach

.PHONY: cargo-deps
cargo-deps: up ## Reinstall cargo dependencies
	${dcrust} cargo update

target/debug: up src ## Compile
	${dcrust} cargo build

.PHONY: rust
rust: up ## Enter an interactive shell into the rust container
	${dcrust} bash

.PHONY: pandoc
pandoc: up ## Enter an interactive shell into the pandoc container
	${dcpandoc} bash

.PHONY: serve
serve: up target/debug ## Serve the compiled application
	${dcpandoc} target/debug/md-to-pdf

.PHONY: fmt
fmt: up ## Format the rust code
	${dcrust} cargo fmt

.PHONY: check
check: up ## Type-check and lint the rust code
	${dcrust} cargo check
	${dcrust} cargo clippy -- -D warnings

.PHONY: test
test: ## Issue a dummy request against the API
	./test.sh

.PHONY: test-api
test-api: ## Run the full API integration suite against a running server
	./test_api.sh

.PHONY: logs
logs: ## Show dev logs
	docker compose logs -f

.PHONY: logs-prod
logs-prod: ## Show production logs
	docker compose -f docker-compose.prod.yml logs -f

.PHONY: down
down: ## Stop dev containers
	docker compose down
