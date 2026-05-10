import { createMCPClient, type MCPClient } from "@ai-sdk/mcp";

let mcpClient: MCPClient | null = null;

export async function getMcpClient(): Promise<MCPClient> {
  if (mcpClient) return mcpClient;

  const url = process.env.AGENT_MEMORY_MCP_URL || "http://127.0.0.1:8080/sse";

  mcpClient = await createMCPClient({
    transport: {
      type: "sse",
      url,
    },
  });

  console.log(`[mcp] connected to agent-memory at ${url}`);
  return mcpClient;
}

export async function closeMcpClient(): Promise<void> {
  if (mcpClient) {
    await mcpClient.close();
    mcpClient = null;
    console.log("[mcp] disconnected");
  }
}

export async function getMcpTools() {
  const client = await getMcpClient();
  return client.tools();
}
