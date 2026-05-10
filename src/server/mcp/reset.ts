import neo4j from "neo4j-driver";

export async function clearNeo4jDatabase(): Promise<void> {
  const uri = process.env.NEO4J_URI || "bolt://localhost:7687";
  const user = process.env.NEO4J_USER || "neo4j";
  const password = process.env.NEO4J_PASSWORD || "password";

  const driver = neo4j.driver(uri, neo4j.auth.basic(user, password));
  const session = driver.session();

  try {
    await session.run("MATCH (n) DETACH DELETE n");
    console.log("[reset] Neo4j database cleared");
  } finally {
    await session.close();
    await driver.close();
  }
}
