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

import { v4 as uuidv4 } from "uuid";
import { Neo4jClient } from "./neo4j";
import { Embedder, getEmbedder } from "./embedder";
import type { ReasoningTrace, ReasoningStep, ToolCall } from "./types";

export class ReasoningMemory {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async startTrace(
    sessionId: string,
    task: string,
    options?: {
      generateEmbedding?: boolean;
      metadata?: Record<string, unknown>;
    },
  ): Promise<ReasoningTrace> {
    const { generateEmbedding = true, metadata } = options || {};
    const traceId = uuidv4();

    let taskEmbedding: number[] | undefined;
    if (generateEmbedding) {
      taskEmbedding = await this.embedder.embed(task);
    }

    await this.client.executeWrite(
      `CREATE (rt:ReasoningTrace {
         id: $id, session_id: $sessionId, task: $task,
         task_embedding: $embedding, outcome: null, success: null,
         completed_at: null, started_at: datetime(), metadata: $metadata
       })`,
      {
        id: traceId,
        sessionId,
        task,
        embedding: taskEmbedding || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: traceId,
      sessionId,
      task,
      taskEmbedding,
      steps: [],
      startedAt: new Date(),
      metadata: metadata || {},
    };
  }

  async addStep(
    traceId: string,
    options?: {
      thought?: string;
      action?: string;
      observation?: string;
      generateEmbedding?: boolean;
      metadata?: Record<string, unknown>;
    },
  ): Promise<ReasoningStep> {
    const { thought, action, observation, generateEmbedding = true, metadata } = options || {};
    const stepId = uuidv4();

    const countRows = await this.client.executeRead(
      `MATCH (:ReasoningTrace {id: $traceId})-[:HAS_STEP]->(s:ReasoningStep)
       RETURN count(s) AS count`,
      { traceId },
    );
    const stepNumber = ((countRows[0]?.count as number) || 0) + 1;

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      const parts: string[] = [];
      if (thought) parts.push(`Thought: ${thought}`);
      if (action) parts.push(`Action: ${action}`);
      if (observation) parts.push(`Observation: ${observation}`);
      if (parts.length > 0) {
        embedding = await this.embedder.embed(parts.join(" "));
      }
    }

    await this.client.executeWrite(
      `MATCH (rt:ReasoningTrace {id: $traceId})
       CREATE (rt)-[:HAS_STEP]->(s:ReasoningStep {
         id: $id, step_number: $stepNumber, thought: $thought,
         action: $action, observation: $observation,
         embedding: $embedding, timestamp: datetime(), metadata: $metadata
       })`,
      {
        traceId,
        id: stepId,
        stepNumber,
        thought: thought || null,
        action: action || null,
        observation: observation || null,
        embedding: embedding || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: stepId,
      traceId,
      stepNumber,
      thought,
      action,
      observation,
      embedding,
      metadata: metadata || {},
    };
  }

  async recordToolCall(
    stepId: string,
    toolName: string,
    args: Record<string, unknown>,
    options?: {
      result?: unknown;
      status?: "pending" | "success" | "failure";
      durationMs?: number;
      error?: string;
    },
  ): Promise<ToolCall> {
    const { result, status = "success", durationMs, error } = options || {};
    const callId = uuidv4();

    await this.client.executeWrite(
      `MATCH (s:ReasoningStep {id: $stepId})
       CREATE (s)-[:HAS_TOOL_CALL]->(tc:ToolCall {
         id: $id, tool_name: $toolName, arguments: $args,
         result: $result, status: $status, duration_ms: $durationMs,
         error: $error, created_at: datetime()
       })`,
      {
        stepId,
        id: callId,
        toolName,
        args: JSON.stringify(args),
        result: result != null ? JSON.stringify(result) : null,
        status,
        durationMs: durationMs || null,
        error: error || null,
      },
    );

    return {
      id: callId,
      stepId,
      toolName,
      arguments: args,
      result,
      status,
      durationMs,
      error,
    };
  }

  async completeTrace(
    traceId: string,
    options?: { outcome?: string; success?: boolean },
  ): Promise<void> {
    const { outcome, success } = options || {};
    await this.client.executeWrite(
      `MATCH (rt:ReasoningTrace {id: $id})
       SET rt.outcome = $outcome, rt.success = $success, rt.completed_at = datetime()`,
      {
        id: traceId,
        outcome: outcome || null,
        success: success ?? null,
      },
    );
  }

  async getSimilarTraces(
    task: string,
    options?: {
      limit?: number;
      successOnly?: boolean;
      threshold?: number;
    },
  ): Promise<Array<ReasoningTrace & { similarity: number }>> {
    const { limit = 5, successOnly = true, threshold = 0.7 } = options || {};
    const taskEmbedding = await this.embedder.embed(task);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('task_embedding_idx', $limit, $embedding)
       YIELD node AS rt, score
       WHERE score >= $threshold AND ($successOnly = false OR rt.success = true)
       RETURN rt, score ORDER BY score DESC`,
      {
        embedding: taskEmbedding,
        limit,
        threshold,
        successOnly,
      },
    );

    return rows.map((r) => {
      const rt = r.rt as Record<string, unknown>;
      return {
        id: rt.id as string,
        sessionId: rt.session_id as string,
        task: rt.task as string,
        taskEmbedding: rt.task_embedding as number[] | undefined,
        outcome: rt.outcome as string | undefined,
        success: rt.success as boolean | undefined,
        startedAt: new Date((rt.started_at as string) || Date.now()),
        completedAt: rt.completed_at ? new Date(rt.completed_at as string) : undefined,
        similarity: r.score as number,
        steps: [],
        metadata: { similarity: r.score as number },
      };
    });
  }

  async searchSteps(
    query: string,
    options?: {
      limit?: number;
      successOnly?: boolean;
      threshold?: number;
    },
  ): Promise<Array<{ step: ReasoningStep; similarity: number; parentTask: string }>> {
    const { limit = 10, successOnly = true, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('step_embedding_idx', $limit, $embedding)
       YIELD node AS rs, score
       WHERE score >= $threshold
       MATCH (rt:ReasoningTrace)-[:HAS_STEP]->(rs)
       WHERE ($successOnly = false OR rt.success = true)
       RETURN rs, score, rt.task AS task, rt.id AS trace_id
       ORDER BY score DESC
       LIMIT $limit`,
      {
        embedding: queryEmbedding,
        limit,
        threshold,
        successOnly,
      },
    );

    return rows.map((row) => {
      const rs = row.rs as Record<string, unknown>;
      return {
        step: {
          id: rs.id as string,
          traceId: (row.trace_id as string) || "unknown",
          stepNumber: (rs.step_number as number) || 0,
          thought: rs.thought as string | undefined,
          action: rs.action as string | undefined,
          observation: rs.observation as string | undefined,
          embedding: rs.embedding as number[] | undefined,
          metadata: {},
        },
        similarity: row.score as number,
        parentTask: (row.task as string) || "unknown",
      };
    });
  }
}
