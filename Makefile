.PHONY: help install server console lint format \
        neo4j-start neo4j-stop neo4j-wait neo4j-status neo4j-logs neo4j-restart neo4j-clean \
        dev dev-stop reset clean clean-all

# MSYS2 bash can't resolve Windows-style paths inside the npm/npx
# bash-script wrappers; use the .cmd launcher on Windows instead.
ifeq ($(OS),Windows_NT)
  NPM := npm.cmd
else
  NPM := npm
endif

# Default target
help:
	@echo "Elysian Dialogue — Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install          Install npm dependencies"
	@echo ""
	@echo "Neo4j (Docker):"
	@echo "  make neo4j-start      Start Neo4j test container"
	@echo "  make neo4j-stop       Stop Neo4j test container"
	@echo "  make neo4j-wait       Wait for Neo4j to be ready"
	@echo "  make neo4j-status     Check Neo4j container status"
	@echo "  make neo4j-logs       Tail Neo4j container logs"
	@echo "  make neo4j-restart    Restart Neo4j container"
	@echo "  make neo4j-clean      Stop container and remove volumes"
	@echo ""
	@echo "Elysian Server & Client:"
	@echo "  make server           Start Express server (:3000)"
	@echo "  make console          Start console REPL client"
	@echo "  make lint             TypeScript type-check (tsc --noEmit) and unused check by knip"
	@echo "  make format           Format source with Prettier"
	@echo ""
	@echo "Full Dev Environment:"
	@echo "  make dev              Start Neo4j + Server (foreground)"
	@echo "  make dev-stop         Stop Neo4j container"
	@echo ""
	@echo "Maintenance:"
	@echo "  make reset            Clear DB, re-seed Neo4j (server must be running)"
	@echo "  make clean            Remove build artifacts"
	@echo "  make clean-all        Remove build artifacts + node_modules"

install:
	$(NPM) install

NEO4J_COMPOSE := docker compose -f docker-compose.test.yml

neo4j-start:
	$(NEO4J_COMPOSE) up -d
	@echo "Neo4j starting... use 'make neo4j-wait' to wait for readiness"

neo4j-stop:
	$(NEO4J_COMPOSE) down

neo4j-restart: neo4j-stop neo4j-start neo4j-wait

neo4j-status:
	@$(NEO4J_COMPOSE) ps -a

neo4j-logs:
	$(NEO4J_COMPOSE) logs -f

neo4j-wait:
	@echo "Waiting for Neo4j to be ready..."
	@$(NEO4J_COMPOSE) up -d
	@for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do \
		if $(NEO4J_COMPOSE) exec -T neo4j cypher-shell -u neo4j -p 12345678 "RETURN 1" > /dev/null 2>&1; then \
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

server:
	$(NPM) run start:dev

console:
	$(NPM) run console

lint:
	$(NPM) run lint
	$(NPM) run knip

format:
	$(NPM) run format

dev: neo4j-start neo4j-wait server-dev

dev-stop: neo4j-stop

reset:
	curl -X POST http://localhost:3000/api/reset

clean:
	rm -rf dist

clean-all: clean
	rm -rf node_modules
