.PHONY: help install install-all neo4j-start neo4j-stop neo4j-wait neo4j-status neo4j-clean neo4j-restart \
        server console lint typecheck clean clean-all \
        dev dev-stop check reset

# Default target
help:
	@echo "Elysian Dialogue — Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install          Install root (npm) + agent-memory (uv) dependencies"
	@echo "  make install-root     Install root npm dependencies only"
	@echo "  make install-am       Install agent-memory uv dependencies only"
	@echo ""
	@echo "Neo4j (Docker):"
	@echo "  make neo4j-start      Start Neo4j test container"
	@echo "  make neo4j-stop       Stop Neo4j test container"
	@echo "  make neo4j-wait       Wait for Neo4j to be ready"
	@echo "  make neo4j-status     Check Neo4j container status"
	@echo "  make neo4j-restart    Restart Neo4j container"
	@echo "  make neo4j-clean      Stop container and remove volumes"
	@echo ""
	@echo "Elysian Server & Client:"
	@echo "  make server           Start Express server (:3000)"
	@echo "  make console          Start console REPL client"
	@echo "  make lint             TypeScript type-check"
	@echo ""
	@echo "Full Dev Environment:"
	@echo "  make dev              Start Neo4j + Server (foreground)"
	@echo "  make dev-stop         Stop Neo4j container"
	@echo ""
	@echo "Maintenance:"
	@echo "  make reset            Clear DB, re-seed Neo4j (server must be running)"
	@echo "  make clean            Remove build artifacts"
	@echo "  make clean-all        Remove build artifacts + node_modules"

# =============================================================================
# Setup
# =============================================================================

install: install-root install-am
	@echo "All dependencies installed!"

install-root:
	npm install

install-am:
	uv sync

# =============================================================================
# Neo4j Docker Management
# =============================================================================

NEO4J_COMPOSE := docker compose -f docker-compose.test.yml

neo4j-start:
	$(NEO4J_COMPOSE) up -d
	@echo "Neo4j starting... use 'make neo4j-wait' to wait for readiness"

neo4j-stop:
	$(NEO4J_COMPOSE) down

neo4j-restart: neo4j-stop neo4j-start neo4j-wait

neo4j-status:
	@$(NEO4J_COMPOSE) ps

neo4j-logs:
	$(NEO4J_COMPOSE) logs -f

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

neo4j-clean:
	$(NEO4J_COMPOSE) down -v
	@echo "Neo4j container and volumes removed"

# =============================================================================
# Elysian Server & Console Client
# =============================================================================

server:
	npm start

console:
	npm run console

# =============================================================================
# Code Quality
# =============================================================================

lint:
	npm run lint

typecheck: lint

# Agent-memory code quality (Python)
lint-am:
	uv run ruff check agent-memory/src agent-memory/tests

format-am:
	uv run ruff format agent-memory/src agent-memory/tests

typecheck-am:
	uv run mypy agent-memory/src

check-am: lint-am format-am typecheck-am
	@echo "All agent-memory code quality checks passed!"

# Full check (both projects)
check: lint
	@echo "All code quality checks passed!"

# =============================================================================
# Full Dev Environment
# =============================================================================

dev: neo4j-start neo4j-wait server

dev-stop: neo4j-stop

# =============================================================================
# Maintenance
# =============================================================================

reset:
	curl -X POST http://localhost:3000/api/reset

clean:
	rm -rf dist
	rm -rf .pytest_cache .mypy_cache .ruff_cache
	rm -rf htmlcov .coverage
	find . -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true

clean-all: clean
	rm -rf node_modules
	rm -rf agent-memory/.venv
