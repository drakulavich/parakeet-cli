.PHONY: install check test unit integration rust-test lint lint-tsgo versions smoke-test smoke-test-tts benchmark release release-preflight release-notes help

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-15s %s\n", $$1, $$2}'

install: ## Install dependencies
	bun install

test: unit integration ## Run all tests

check: lint versions test ## Run local checks that mirror the cheap CI gates

unit: ## Run unit tests
	bun run test:unit

integration: ## Run integration tests
	bun run test:integration

rust-test: ## Run Rust tests via nextest (matches CI — rust-test.yml)
	cd rust && cargo nextest run --features tts

lint: ## Type-check with tsc
	bunx tsc --noEmit

lint-tsgo: ## Type-check with tsgo (TypeScript 7 native preview, advisory)
	bunx tsgo --noEmit

versions: ## Check version drift between package.json + Cargo.toml (#267 F16)
	bun .github/scripts/check-versions.ts

smoke-test: ## Run smoke tests against fixtures
	bun link @drakulavich/kesha-voice-kit
	kesha install
	bun scripts/smoke-test.ts

smoke-test-tts: ## Run smoke tests with TTS
	bun link @drakulavich/kesha-voice-kit
	kesha install --tts
	bun scripts/smoke-test.ts --tts

benchmark: ## Run benchmark (openai-whisper vs faster-whisper vs Kesha)
	bun scripts/benchmark.ts

release-preflight: check smoke-test ## Verify locally before cutting a GitHub release
	@echo "Release preflight passed. Cut/publish via the GitHub release workflow, not npm publish."

release: release-preflight ## Backward-compatible alias for release-preflight

release-notes: ## Print an existing release body: make release-notes TAG=vX.Y.Z
	@test -n "$(TAG)" || (echo "usage: make release-notes TAG=vX.Y.Z" >&2; exit 2)
	gh release view "$(TAG)" --json body --jq .body
