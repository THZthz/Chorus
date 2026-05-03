import db from "@/server/db";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { WorldSnapshot } from "@/types/entities";
import { v4 as uuidv4 } from "uuid";

export interface DialogueStepRow {
  id: string;
  parent_step_id: string | null;
  parent_option_id: string | null;
  messages: string; // JSON
  options: string; // JSON
  world_snapshot: string | null;
  is_generated: number;
  is_active: number;
  created_at: string;
}

export interface DialogueStepParsed {
  id: string;
  parentStepId: string | null;
  parentOptionId: string | null;
  messages: Message[];
  options: DialogueOption[];
  worldSnapshot: WorldSnapshot | null;
  isGenerated: boolean;
  isActive: boolean;
  createdAt: string;
}

function parseStep(row: DialogueStepRow): DialogueStepParsed {
  return {
    id: row.id,
    parentStepId: row.parent_step_id,
    parentOptionId: row.parent_option_id,
    messages: JSON.parse(row.messages),
    options: JSON.parse(row.options),
    worldSnapshot: row.world_snapshot ? JSON.parse(row.world_snapshot) : null,
    isGenerated: row.is_generated === 1,
    isActive: row.is_active === 1,
    createdAt: row.created_at,
  };
}

export function getStep(id: string): DialogueStepParsed | null {
  const row = db.prepare("SELECT * FROM dialogue_steps WHERE id = ?").get(id) as
    | DialogueStepRow
    | undefined;
  return row ? parseStep(row) : null;
}

export function saveStep(step: {
  id: string;
  parentStepId?: string | null;
  parentOptionId?: string | null;
  messages: Message[];
  options: DialogueOption[];
  worldSnapshot?: WorldSnapshot | null;
  isGenerated?: boolean;
  isActive?: boolean;
}): void {
  db.prepare(
    `
    INSERT OR REPLACE INTO dialogue_steps (id, parent_step_id, parent_option_id, messages, options, world_snapshot, is_generated, is_active)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
  `,
  ).run(
    step.id,
    step.parentStepId ?? null,
    step.parentOptionId ?? null,
    JSON.stringify(step.messages),
    JSON.stringify(step.options),
    step.worldSnapshot ? JSON.stringify(step.worldSnapshot) : null,
    step.isGenerated ? 1 : 0,
    step.isActive !== false ? 1 : 0,
  );
}

export function getBranchPath(stepId: string): DialogueStepParsed[] {
  const steps: DialogueStepParsed[] = [];
  let current: DialogueStepParsed | null = getStep(stepId);
  while (current) {
    steps.unshift(current);
    current = current.parentStepId ? getStep(current.parentStepId) : null;
  }
  return steps;
}

export function getChildSteps(parentStepId: string): DialogueStepParsed[] {
  const rows = db
    .prepare("SELECT * FROM dialogue_steps WHERE parent_step_id = ? ORDER BY created_at ASC")
    .all(parentStepId) as DialogueStepRow[];
  return rows.map(parseStep);
}

export function setBranchActive(stepId: string, active: boolean): void {
  db.prepare("UPDATE dialogue_steps SET is_active = ? WHERE id = ?").run(active ? 1 : 0, stepId);
}

export function deactivateSiblingBranches(parentStepId: string, exceptStepId: string): void {
  db.prepare("UPDATE dialogue_steps SET is_active = 0 WHERE parent_step_id = ? AND id != ?").run(
    parentStepId,
    exceptStepId,
  );
}

// Alternatives (regenerate/swipe support)

export interface AlternativeRow {
  id: string;
  step_id: string;
  messages: string;
  options: string;
  sequence_num: number;
  created_at: string;
}

export function saveAlternative(
  stepId: string,
  messages: Message[],
  options: DialogueOption[],
): string {
  const id = uuidv4();
  const maxSeq = db
    .prepare("SELECT MAX(sequence_num) as max_seq FROM dialogue_alternatives WHERE step_id = ?")
    .get(stepId) as { max_seq: number | null };
  const nextSeq = (maxSeq?.max_seq ?? -1) + 1;

  // Shift all existing alternatives up
  db.prepare(
    "UPDATE dialogue_alternatives SET sequence_num = sequence_num + 1 WHERE step_id = ?",
  ).run(stepId);

  db.prepare(
    `
    INSERT INTO dialogue_alternatives (id, step_id, messages, options, sequence_num)
    VALUES (?, ?, ?, ?, 0)
  `,
  ).run(id, stepId, JSON.stringify(messages), JSON.stringify(options));

  console.log(
    `[dialogue] saved alternative id=${id} stepId=${stepId} seq=0 prevMax=${nextSeq - 1}`,
  );

  return id;
}

export function getAlternatives(
  stepId: string,
): { id: string; messages: Message[]; options: DialogueOption[]; sequenceNum: number }[] {
  const rows = db
    .prepare("SELECT * FROM dialogue_alternatives WHERE step_id = ? ORDER BY sequence_num ASC")
    .all(stepId) as AlternativeRow[];
  return rows.map((r) => ({
    id: r.id,
    messages: JSON.parse(r.messages),
    options: JSON.parse(r.options),
    sequenceNum: r.sequence_num,
  }));
}

export function setCurrentAlternative(stepId: string, alternativeId: string): void {
  const alt = db.prepare("SELECT * FROM dialogue_alternatives WHERE id = ?").get(alternativeId) as
    | AlternativeRow
    | undefined;
  if (!alt) return;

  db.transaction(() => {
    // Shift all alternatives so the chosen one becomes sequence_num=0
    db.prepare(
      "UPDATE dialogue_alternatives SET sequence_num = sequence_num + 1 WHERE step_id = ?",
    ).run(stepId);
    db.prepare("UPDATE dialogue_alternatives SET sequence_num = 0 WHERE id = ?").run(alternativeId);

    // Update the step's messages and options to the alternative
    db.prepare("UPDATE dialogue_steps SET messages = ?, options = ? WHERE id = ?").run(
      alt.messages,
      alt.options,
      stepId,
    );
  })();
}

// ── Tree navigation ──

export function getRootStep(): DialogueStepParsed | null {
  const row = db
    .prepare(
      "SELECT * FROM dialogue_steps WHERE parent_step_id IS NULL ORDER BY created_at ASC LIMIT 1",
    )
    .get() as DialogueStepRow | undefined;
  return row ? parseStep(row) : null;
}

export function getLeafSteps(): DialogueStepParsed[] {
  const rows = db
    .prepare(
      `
    SELECT * FROM dialogue_steps ds
    WHERE ds.is_active = 1
      AND NOT EXISTS (
        SELECT 1 FROM dialogue_steps child
        WHERE child.parent_step_id = ds.id AND child.is_active = 1
      )
    ORDER BY ds.created_at ASC
  `,
    )
    .all() as DialogueStepRow[];
  return rows.map(parseStep);
}

export function getLatestLeafStep(): DialogueStepParsed | null {
  const row = db
    .prepare(
      `
    SELECT * FROM dialogue_steps ds
    WHERE ds.is_active = 1
      AND NOT EXISTS (
        SELECT 1 FROM dialogue_steps child
        WHERE child.parent_step_id = ds.id AND child.is_active = 1
      )
    ORDER BY ds.created_at DESC LIMIT 1
  `,
    )
    .get() as DialogueStepRow | undefined;
  return row ? parseStep(row) : null;
}

export function getChildByOption(
  parentStepId: string,
  parentOptionId: string,
): DialogueStepParsed | null {
  const row = db
    .prepare(
      "SELECT * FROM dialogue_steps WHERE parent_step_id = ? AND parent_option_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .get(parentStepId, parentOptionId) as DialogueStepRow | undefined;
  return row ? parseStep(row) : null;
}

export function getAllActiveSteps(): DialogueStepParsed[] {
  const rows = db
    .prepare("SELECT * FROM dialogue_steps WHERE is_active = 1 ORDER BY created_at ASC")
    .all() as DialogueStepRow[];
  return rows.map(parseStep);
}

export function getAllSteps(): DialogueStepParsed[] {
  const rows = db
    .prepare("SELECT * FROM dialogue_steps ORDER BY created_at ASC")
    .all() as DialogueStepRow[];
  return rows.map(parseStep);
}

export function updateOptionNextStepId(stepId: string, optionId: string, nextStepId: string): void {
  const step = getStep(stepId);
  if (!step) return;
  const updatedOptions = step.options.map((opt) =>
    opt.id === optionId ? { ...opt, nextStepId } : opt,
  );
  db.prepare("UPDATE dialogue_steps SET options = ? WHERE id = ?").run(
    JSON.stringify(updatedOptions),
    stepId,
  );
}

export function updateStepSnapshot(id: string, worldSnapshot: WorldSnapshot): boolean {
  const existing = db
    .prepare("SELECT id FROM dialogue_steps WHERE id = ?")
    .get(id) as { id: string } | undefined;
  if (!existing) return false;
  db.prepare("UPDATE dialogue_steps SET world_snapshot = ? WHERE id = ?").run(
    JSON.stringify(worldSnapshot),
    id,
  );
  return true;
}

export function getTreeStats(): {
  totalSteps: number;
  rootId: string | null;
  leafIds: string[];
  branchCount: number;
} {
  const root = getRootStep();
  const leaves = getLeafSteps();
  const countRow = db
    .prepare("SELECT COUNT(*) as count FROM dialogue_steps WHERE is_active = 1")
    .get() as { count: number };

  const branchesRow = db
    .prepare(
      `SELECT COUNT(DISTINCT parent_step_id) as count FROM dialogue_steps
       WHERE is_active = 1 AND parent_step_id IS NOT NULL`,
    )
    .get() as { count: number };

  return {
    totalSteps: countRow.count,
    rootId: root?.id ?? null,
    leafIds: leaves.map((l) => l.id),
    branchCount: branchesRow.count,
  };
}
