.PHONY: install server console lint format test-all \
        neo4j-start neo4j-stop neo4j-wait neo4j-status neo4j-logs neo4j-restart neo4j-clean \
        embedding-server rerank-server formatter-server \
        reset clean clean-all

# MSYS2 bash can't resolve Windows-style paths inside the npm/npx
# bash-script wrappers; use the .cmd launcher on Windows instead.
ifeq ($(OS),Windows_NT)
  NPM := npm.cmd
else
  NPM := npm
endif

install:
	$(NPM) install

NEO4J_COMPOSE := docker compose -f docker-compose.yml

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

embedding-server:
	llama-server -m data/models/Qwen3-Embedding-0.6B-Q8_0.gguf --port 8080 -c 32768 -ngl 99 --embeddings

rerank-server:
	llama-server -m data/models/Qwen3-Reranker-0.6B-Q8_0.gguf --port 8081 -c 32768 -ngl 99 --reranking

formatter-server:
	llama-server -m data/models/Qwen3.5-9B-Q4_K_M.gguf --port 8082 -c 32768 -ngl 99 --jinja
	# llama-server -m data/models/Qwen3.5-9B-Q4_K_M.gguf --port 8082 -c 32768 -ngl 99 --jinja --chat-template-kwargs '{"enable_thinking": false}'

console:
	$(NPM) run console

lint:
	$(NPM) run lint
	$(NPM) run knip

format:
	$(NPM) run format
	$(NPM) run add-license-header

test-all:
	$(NPM) test

reset:
	curl -X POST http://localhost:3000/api/reset

clean:
	rm -rf dist

clean-all: clean
	rm -rf node_modules
