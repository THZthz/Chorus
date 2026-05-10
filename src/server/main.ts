import "dotenv/config";
import express from "express";
import apiRouter from "@/server/api";
import { getMcpClient, closeMcpClient } from "@/server/mcp/client";
import { seedDatabase } from "@/server/mcp/seed";

async function start() {
  const app = express();
  const PORT = 3000;

  app.use(express.json());
  app.use("/api", apiRouter);

  // Initialize MCP connection (stays alive for server lifetime)
  await getMcpClient();

  // Seed Neo4j with initial world data
  await seedDatabase();

  app.listen(PORT, "0.0.0.0", () => {
    console.log(`Server running on http://localhost:${PORT}`);
  });

  const shutdown = async () => {
    console.log("\nShutting down...");
    await closeMcpClient();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

start();
