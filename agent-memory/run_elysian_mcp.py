"""Start the agent-memory MCP server configured for Elysian Dialogue."""
import asyncio
import os
from neo4j_agent_memory.mcp.server import run_server

if __name__ == "__main__":
    asyncio.run(run_server(
        neo4j_uri=os.environ.get("NEO4J_URI", "bolt://localhost:7687"),
        neo4j_user=os.environ.get("NEO4J_USER", "neo4j"),
        neo4j_password=os.environ.get("NEO4J_PASSWORD", "password"),
        neo4j_database=os.environ.get("NEO4J_DATABASE", "neo4j"),
        transport="sse",
        host="127.0.0.1",
        port=8080,
        profile="extended",
        session_strategy="persistent",
        user_id="elysian-game",
        observation_threshold=50000,
        auto_preferences=False,
    ))
