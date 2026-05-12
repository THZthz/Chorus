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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { getGameState, saveGameState } from "@/server/memory/gameState";

function getClient(): MemoryClient {
  return MemoryClient.getCachedInstance();
}

function sanitizeInt(n: number, fallback: number = 1): number {
  if (typeof n !== "number" || !Number.isFinite(n)) return fallback;
  const int = Math.floor(n);
  return int >= 0 ? int : fallback;
}

function sanitizeConfidence(n: number): number {
  if (typeof n !== "number" || !Number.isFinite(n)) return 1.0;
  if (n < 1e-6) return 0;
  if (n > 1) return 1;
  return n;
}

export function createMemoryTools() {

  // ── Scene ──

  const getScene = tool({
    description:
      "Get everything in-frame around the player right now: current location, NPCs present, objects, inventory, and active plot beats. Call this FIRST every turn to understand the current situation.",
    inputSchema: z.object({
      playerName: z.string().default("Player").describe("Player entity name"),
    }),
    execute: async (input) => {
      const client = getClient();
      const rows = await client.executeReadOnlyCypher(
        `MATCH (player:Entity {name: $playerName})
         OPTIONAL MATCH (player)-[:LOCATED_AT]->(loc:Entity)
         OPTIONAL MATCH (npc:Entity)-[:LOCATED_AT]->(loc)
           WHERE npc.type = "PERSON" AND npc.name <> $playerName
         OPTIONAL MATCH (obj:Entity)-[:LOCATED_AT]->(loc)
           WHERE obj.type = "OBJECT"
         OPTIONAL MATCH (player)-[:CARRIES]->(inv:Entity)
         OPTIONAL MATCH (player)-[:LOCATED_AT]->(loc2:Entity)-[:LOCATED_IN]->(parent:Entity)
         OPTIONAL MATCH (plot:Entity)
           WHERE plot.type = "EVENT" AND (plot.status IS NULL OR plot.status IN ["PENDING", "IN_PROGRESS"])
         WITH player, loc, collect(DISTINCT npc) AS npcs,
              collect(DISTINCT obj) AS objects,
              collect(DISTINCT inv) AS inventory,
              collect(DISTINCT parent) AS parents,
              collect(DISTINCT plot) AS activePlots
         RETURN {
           player: player {.name, .type, .subtype, .description, .metadata},
           location: loc {.name, .type, .subtype, .description, .metadata},
           presentNPCs: [npc IN npcs | npc {.name, .type, .subtype, .description, .metadata}],
           presentObjects: [obj IN objects | obj {.name, .type, .subtype, .description, .metadata}],
           inventory: [item IN inventory | item {.name, .type, .subtype, .description, .metadata}],
           parentLocations: [p IN parents | p {.name, .type, .subtype}],
           activePlots: [p IN activePlots | p {.name, .description, .metadata, .status}]
         } AS scene`,
        { playerName: input.playerName },
      );
      const scene = rows.length > 0 ? rows[0]?.scene : null;
      return JSON.stringify(scene ?? { error: "Player entity not found" }, null, 2);
    },
  });

  // ── World Mutation ──

  const updateWorld = tool({
    description:
      "Change the game world. Use action to specify what kind of change:\n" +
      '- "move": Move an entity to a location. Fields: entityName, targetLocation.\n' +
      '- "change": Update an entity\'s description or metadata. Fields: entityName, description?, metadata?.\n' +
      '- "create": Create a new entity. Fields: name, entityType (PERSON/OBJECT/LOCATION/ORGANIZATION/EVENT), subtype?, description?, metadata?.\n' +
      '- "relate": Create a relationship between entities. Fields: sourceName, targetName, relationshipType (UPPER_SNAKE_CASE: LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS, etc.), description?.\n' +
      '- "fact": Record a fact triple. Fields: subject, predicate, objectValue.',
    inputSchema: z.object({
      action: z.enum(["move", "change", "create", "relate", "fact"]).describe("Type of world change"),
      entityName: z.string().optional().describe("Entity name (move, change)"),
      targetLocation: z.string().optional().describe("Target location name (move)"),
      name: z.string().optional().describe("Entity name (create)"),
      entityType: z.enum(["PERSON", "OBJECT", "LOCATION", "ORGANIZATION", "EVENT"]).optional().describe("POLE+O type (create)"),
      subtype: z.string().optional().describe("Optional subtype (create)"),
      description: z.string().optional().describe("Entity description (create, change)"),
      metadata: z.record(z.string(), z.unknown()).optional().describe("Entity metadata (create, change)"),
      sourceName: z.string().optional().describe("Source entity (relate)"),
      targetName: z.string().optional().describe("Target entity (relate)"),
      relationshipType: z.string().optional().describe("Relationship type in UPPER_SNAKE_CASE (relate)"),
      subject: z.string().optional().describe("Fact subject (fact)"),
      predicate: z.string().optional().describe("Fact predicate (fact)"),
      objectValue: z.string().optional().describe("Fact object (fact)"),
    }),
    execute: async (input) => {
      const client = getClient();
      switch (input.action) {
        case "move": {
          if (!input.entityName || !input.targetLocation) {
            return JSON.stringify({ error: "entityName and targetLocation required for move" });
          }
          // Remove old LOCATED_AT and create new one
          await client.neo4j.executeWrite(
            `MATCH (e:Entity {name: $entityName})-[old:LOCATED_AT]->(:Entity)
             DELETE old`,
            { entityName: input.entityName },
          );
          await client.longTerm.addRelationship(input.entityName, input.targetLocation, "LOCATED_AT");
          client.observer.onWorldChange({ action: "move", summary: `${input.entityName} → ${input.targetLocation}` });
          return JSON.stringify({ moved: input.entityName, to: input.targetLocation });
        }
        case "change": {
          if (!input.entityName) {
            return JSON.stringify({ error: "entityName required for change" });
          }
          const entity = await client.longTerm.addEntity(input.entityName, "PERSON", {
            description: input.description,
            metadata: input.metadata as Record<string, unknown> | undefined,
          });
          client.observer.onWorldChange({ action: "change", summary: `Updated ${entity.name}` });
          return JSON.stringify({ updated: entity.name, id: entity.id });
        }
        case "create": {
          if (!input.name || !input.entityType) {
            return JSON.stringify({ error: "name and entityType required for create" });
          }
          const entity = await client.longTerm.addEntity(input.name, input.entityType, {
            subtype: input.subtype,
            description: input.description,
            metadata: input.metadata as Record<string, unknown> | undefined,
          });
          client.observer.onWorldChange({ action: "create", summary: `Created ${entity.type} ${entity.name}` });
          return JSON.stringify({ created: entity.name, id: entity.id, type: entity.type });
        }
        case "relate": {
          if (!input.sourceName || !input.targetName || !input.relationshipType) {
            return JSON.stringify({ error: "sourceName, targetName, relationshipType required for relate" });
          }
          const result = await client.longTerm.addRelationship(
            input.sourceName, input.targetName, input.relationshipType,
            { description: input.description },
          );
          client.observer.onWorldChange({ action: "relate", summary: `${input.sourceName} --[${input.relationshipType}]--> ${input.targetName}` });
          return JSON.stringify({ related: true, created: result.created });
        }
        case "fact": {
          if (!input.subject || !input.predicate || !input.objectValue) {
            return JSON.stringify({ error: "subject, predicate, objectValue required for fact" });
          }
          const fact = await client.longTerm.addFact(input.subject, input.predicate, input.objectValue);
          client.observer.onWorldChange({ action: "fact", summary: `${input.subject} ${input.predicate} ${input.objectValue}` });
          return JSON.stringify({ recorded: true, id: fact.id });
        }
      }
    },
  });

  // ── GM Notes ──

  const remember = tool({
    description: "Store a GM note or observation. Use for tracking clues, NPC dispositions, or important events.",
    inputSchema: z.object({
      note: z.string().describe("Note content"),
      about: z.string().optional().describe("Entity name this note is about"),
    }),
    execute: async (input) => {
      const client = getClient();
      const msg = await client.shortTerm.addMessage("system", input.note, {
        about: input.about,
      });
      return JSON.stringify({ remembered: true, id: msg.id });
    },
  });

  // ── Conversation ──

  const getConversation = tool({
    description: "Retrieve recent conversation history.",
    inputSchema: z.object({
      limit: z.number().default(50).describe("Max messages"),
    }),
    execute: async (input) => {
      const client = getClient();
      const messages = await client.shortTerm.getConversation(sanitizeInt(input.limit, 50));
      return JSON.stringify({ messageCount: messages.length, messages }, null, 2);
    },
  });

  // ── Search ──

  const searchMemory = tool({
    description: "Search all world state (entities, facts, messages) by meaning. Use when you need to find something not in the current scene.",
    inputSchema: z.object({
      query: z.string().describe("Natural language search query"),
      limit: z.number().default(10).describe("Max results per type"),
    }),
    execute: async (input) => {
      const client = getClient();
      const results = await client.search.search(input.query, {
        limit: sanitizeInt(input.limit, 10),
      });
      return JSON.stringify(results, null, 2);
    },
  });

  // ── Plot ──

  const advancePlot = tool({
    description: "Update the current story beat. Call when the player's actions complete or advance a plot thread.",
    inputSchema: z.object({
      plotName: z.string().describe("Name of the plot EVENT entity"),
      status: z.enum(["PENDING", "IN_PROGRESS", "RESOLVED", "ABANDONED"]).optional().describe("New plot status"),
      currentBeat: z.string().optional().describe("Description of the current story beat"),
      revealed: z.string().optional().describe("Clue or information revealed to the player"),
    }),
    execute: async (input) => {
      const client = getClient();
      // Update plot entity metadata
      const meta: Record<string, unknown> = {};
      if (input.status) meta.status = input.status;
      if (input.currentBeat) meta.currentBeat = input.currentBeat;
      if (input.revealed) {
        const existing = await client.longTerm.getEntity(input.plotName);
        const existingMeta = existing?.metadata || {};
        const revealedList = (existingMeta.revealedClues as string[]) || [];
        revealedList.push(input.revealed);
        meta.revealedClues = revealedList;
      }

      if (Object.keys(meta).length > 0) {
        await client.longTerm.addEntity(input.plotName, "EVENT", { metadata: meta });
      }

      // Update game state with current plot node
      try {
        const state = await getGameState();
        const options = state?.options || [];
        await saveGameState(state?.id || "step_0", options);
      } catch {
        // game state may not exist yet — fine
      }

      return JSON.stringify({ advanced: input.plotName, status: input.status, beat: input.currentBeat });
    },
  });

  return {
    getScene,
    updateWorld,
    remember,
    getConversation,
    searchMemory,
    advancePlot,
  };
}
