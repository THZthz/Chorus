// Entity types following POLE+O model
export type EntityType = "PERSON" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT";

export type MessageRole = "user" | "assistant" | "system";

export interface MemoryEntity {
  id: string;
  name: string;
  type: EntityType;
  subtype?: string;
  description?: string;
  aliases: string[];
  metadata: Record<string, unknown>;
  embedding?: number[];
  createdAt: Date;
}

export interface MemoryMessage {
  id: string;
  sessionId: string;
  role: MessageRole;
  content: string;
  metadata: Record<string, unknown>;
  embedding?: number[];
  createdAt: Date;
}

export interface MemoryPreference {
  id: string;
  category: string;
  preference: string;
  context?: string;
  confidence: number;
  metadata: Record<string, unknown>;
  createdAt: Date;
}

export interface MemoryFact {
  id: string;
  subject: string;
  predicate: string;
  object: string;
  confidence: number;
  validFrom?: Date;
  validUntil?: Date;
  metadata: Record<string, unknown>;
  createdAt: Date;
}

export interface EntityRelationship {
  id: string;
  sourceId: string;
  targetId: string;
  type: string;
  description?: string;
  confidence: number;
}

export interface ReasoningTrace {
  id: string;
  sessionId: string;
  task: string;
  taskEmbedding?: number[];
  steps: ReasoningStep[];
  outcome?: string;
  success?: boolean;
  startedAt: Date;
  completedAt?: Date;
  metadata: Record<string, unknown>;
}

export interface ReasoningStep {
  id: string;
  traceId: string;
  stepNumber: number;
  thought?: string;
  action?: string;
  observation?: string;
  embedding?: number[];
  metadata: Record<string, unknown>;
}

export interface ToolCall {
  id: string;
  stepId: string;
  toolName: string;
  arguments: Record<string, unknown>;
  result?: unknown;
  status: "pending" | "success" | "failure";
  durationMs?: number;
  error?: string;
}

export interface SessionSummary {
  sessionId: string;
  title?: string;
  messageCount: number;
  createdAt: Date;
  updatedAt?: Date;
  firstMessagePreview?: string;
  lastMessagePreview?: string;
}

export interface Observation {
  type: "fact" | "decision" | "preference" | "topic" | "entity";
  content: string;
  sourceMessageId?: string;
  timestamp: string;
  confidence: number;
}

export interface ObservationResult {
  sessionId: string;
  messageCount: number;
  approximateTokens: number;
  thresholdTokens: number;
  thresholdExceeded: boolean;
  reflections: string[];
  observations: Observation[];
  entityNames: string[];
  topics: string[];
}

export interface SearchResults {
  messages: Array<MemoryMessage & { similarity: number }>;
  entities: Array<MemoryEntity & { similarity: number }>;
  preferences: Array<MemoryPreference & { similarity: number }>;
  traces: Array<ReasoningTrace & { similarity: number }>;
}

export interface AssembledContext {
  messages: MemoryMessage[];
  entities: MemoryEntity[];
  preferences: MemoryPreference[];
  traces: ReasoningTrace[];
  summary: string;
}
