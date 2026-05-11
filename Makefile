.PHONY: help install install-all install-dev lint format typecheck test test-unit test-integration test-integration-mcp test-e2e test-all test-docker test-ci test-no-docker test-quick test-file test-match test-aws coverage coverage-all coverage-ci coverage-mcp test-examples test-examples-quick test-examples-no-neo4j test-docs test-docs-syntax test-docs-build test-docs-links neo4j-start neo4j-stop neo4j-logs clean build publish docs docs-diagrams-list docs-diagrams-status docs-diagrams-missing docs-diagrams-manifest docs-diagrams-add-refs docs-diagrams-generate example-basic example-resolution example-langchain example-pydantic examples chat-agent-install chat-agent-backend chat-agent-frontend chat-agent

# Default target
help:
	@echo "neo4j-agent-memory Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install          Install core dependencies"
	@echo "  make install-all      Install all dependencies including extras"
	@echo "  make install-dev      Install development dependencies"
	@echo ""
	@echo "Code Quality:"
	@echo "  make lint             Run linter (ruff check)"
	@echo "  make format           Format code (ruff format)"
	@echo "  make typecheck        Run type checker (mypy)"
	@echo "  make check            Run all code quality checks"
	@echo ""
	@echo "Neo4j:"
	@echo "  make neo4j-start      Start Neo4j test container"
	@echo "  make neo4j-stop       Stop Neo4j test container"
	@echo "  make neo4j-logs       View Neo4j container logs"
	@echo "  make neo4j-status     Check Neo4j container status"
	@echo "  make neo4j-wait       Wait for Neo4j to be ready"
	@echo "  make neo4j-clean      Stop and remove Neo4j data volumes"
	@echo ""
	@echo "Build & Publish:"
	@echo "  make build            Build package"
	@echo "  make publish          Publish to PyPI (requires credentials)"
	@echo "  make clean            Remove build artifacts"

# =============================================================================
# Setup
# =============================================================================

install:
	uv sync

install-all:
	uv sync --all-extras

install-dev:
	uv sync --extra dev

# =============================================================================
# Code Quality
# =============================================================================

lint:
	uv run ruff check src tests

lint-fix:
	uv run ruff check --fix src tests

format:
	uv run ruff format src tests

format-check:
	uv run ruff format --check src tests

typecheck:
	uv run mypy src

check: lint format-check typecheck
	@echo "All code quality checks passed!"

# =============================================================================
# Neo4j Docker Management
# =============================================================================

NEO4J_COMPOSE := docker compose -f docker-compose.test.yml

neo4j-start:
	$(NEO4J_COMPOSE) up -d
	@echo "Neo4j starting... use 'make neo4j-wait' to wait for it to be ready"

neo4j-stop:
	$(NEO4J_COMPOSE) down

neo4j-restart: neo4j-stop neo4j-start neo4j-wait

neo4j-logs:
	$(NEO4J_COMPOSE) logs -f

neo4j-status:
	@$(NEO4J_COMPOSE) ps

neo4j-wait:
	@echo "Waiting for Neo4j to be ready..."
	@$(NEO4J_COMPOSE) up -d
	@for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do \
		if $(NEO4J_COMPOSE) exec -T neo4j cypher-shell -u neo4j -p test-password "RETURN 1" > /dev/null 2>&1; then \
			echo "Neo4j is ready!"; \
			exit 0; \
		fi; \
		echo "Waiting for Neo4j... ($$i/30)"; \
		sleep 2; \
	done; \
	echo "Neo4j failed to start within 60 seconds"; \
	exit 1

# Quiet version for internal use
neo4j-wait-quiet:
	@for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do \
		if $(NEO4J_COMPOSE) exec -T neo4j cypher-shell -u neo4j -p test-password "RETURN 1" > /dev/null 2>&1; then \
			exit 0; \
		fi; \
		sleep 2; \
	done; \
	echo "Neo4j failed to start"; \
	exit 1

neo4j-clean:
	$(NEO4J_COMPOSE) down -v
	@echo "Neo4j container and volumes removed"

neo4j-shell:
	$(NEO4J_COMPOSE) exec neo4j cypher-shell -u neo4j -p test-password

# =============================================================================
# Build & Publish
# =============================================================================

clean:
	rm -rf dist/
	rm -rf build/
	rm -rf *.egg-info/
	rm -rf src/*.egg-info/
	rm -rf .pytest_cache/
	rm -rf .mypy_cache/
	rm -rf .ruff_cache/
	rm -rf htmlcov/
	rm -rf .coverage
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true

build: clean
	uv build

publish: build
	uv publish

publish-test: build
	uv publish --repository testpypi
