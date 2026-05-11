import { MemoryClient } from "@/server/memory/client";

export async function clearNeo4jDatabase(): Promise<void> {
  const client = await MemoryClient.getInstance();
  await client.neo4j.executeWrite("MATCH (n) DETACH DELETE n");
  console.log("[reset] Neo4j database cleared");
}
