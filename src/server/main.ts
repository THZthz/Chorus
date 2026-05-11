/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

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
