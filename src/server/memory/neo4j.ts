import neo4j from "neo4j-driver";
import type { Driver } from "neo4j-driver";

export class Neo4jClient {
  private driver: Driver;

  constructor(
    uri: string = process.env.NEO4J_URI || "bolt://localhost:7687",
    user: string = process.env.NEO4J_USER || "neo4j",
    password: string = process.env.NEO4J_PASSWORD || "password",
  ) {
    this.driver = neo4j.driver(uri, neo4j.auth.basic(user, password));
  }

  async verifyConnectivity(): Promise<void> {
    await this.driver.verifyConnectivity();
  }

  async executeRead(
    query: string,
    parameters?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    const session = this.driver.session();
    try {
      const result = await session.executeRead((tx) =>
        tx.run(query, parameters),
      );
      return result.records.map((r) => r.toObject());
    } finally {
      await session.close();
    }
  }

  async executeWrite(
    query: string,
    parameters?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    const session = this.driver.session();
    try {
      const result = await session.executeWrite((tx) =>
        tx.run(query, parameters),
      );
      return result.records.map((r) => r.toObject());
    } finally {
      await session.close();
    }
  }

  async close(): Promise<void> {
    await this.driver.close();
  }
}
