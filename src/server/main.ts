/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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
import { seedDatabase } from "@/server/stories/seed";

const NEO4J_RETRY_MS = 3000;

async function initMemory(): Promise<void> {
  // eslint-disable-next-line no-constant-condition
  while (true) {
    try {
      console.log("[memory] connecting to Neo4j...");
      await MemoryClient.getInstance();
      console.log("[memory] connected.");
      return;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(
        `[memory] Neo4j unavailable (${msg}), retrying in ${NEO4J_RETRY_MS / 1000}s...`,
      );
      await new Promise((r) => setTimeout(r, NEO4J_RETRY_MS));
    }
  }
}

async function start() {
  try {
    const app = express();
    const PORT = 3000;

    app.use(express.json());
    app.use("/api", apiRouter);

    // Initialize MemoryClient (retries until Neo4j is available)
    await initMemory();

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

    process.on("uncaughtException", (error) => {
      console.error("[process] uncaughtException — exiting:", error);
      process.exit(1);
    });
    process.on("unhandledRejection", (reason) => {
      console.error("[process] unhandledRejection:", reason);
    });
  } catch (error) {
    console.error("[start] fatal startup error:", error);
    process.exit(1);
  }
}

start();
