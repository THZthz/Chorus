import "dotenv/config";
import express from "express";
import apiRouter from "@/server/api";
import { MemoryClient } from "@/server/memory/client";
import { seedDatabase } from "@/server/mcp/seed";

async function start() {
  const app = express();
  const PORT = 3000;

  app.use(express.json());
  app.use("/api", apiRouter);

  // Initialize MemoryClient (stays alive for server lifetime)
  console.log("[memory] initializing local memory layer...");
  await MemoryClient.getInstance();

  // Seed Neo4j with initial world data
  await seedDatabase();

  app.listen(PORT, "0.0.0.0", () => {
    console.log(`Server running on http://localhost:${PORT}`);
  });

  const shutdown = async () => {
    console.log("\nShutting down...");
    await MemoryClient.closeInstance();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

start();
