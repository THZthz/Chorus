import db from "@/server/db";
import { WorldEntity, WorldState, Character, Location, WorldObject, EntityType } from "@/types/entities";

const initialObjects: Record<string, WorldObject> = {
  rusted_coin: {
    id: "rusted_coin",
    type: "OBJECT",
    displayName: "Rusted Iron Coin",
    shortDescription: "A heavy, notched currency from the Old Duchy.",
    longDescription:
      "The iron is pitted with rust, but the seal of the dead Duke is still visible. It smells of copper and damp earth. In a place like the Crimson Veil, it might buy you a half-measure of watered wine or a very short conversation.",
    attributes: {
      Value: "Near-worthless",
      Origin: "Duchy of Oros",
    },
  },
  velvet_choker: {
    id: "velvet_choker",
    type: "OBJECT",
    displayName: "Stained Velvet Choker",
    shortDescription: "Once elegant, now frayed and smelling of heavy musk.",
    longDescription:
      "The deep red fabric is stiff with age and sweat. It has a small, tarnished silver clasp that looks like a weeping eye. It belongs to Vespera, though she hasn't worn it since the plague years.",
    attributes: {
      Scent: "Musk and Decay",
      Material: "Velvet",
    },
  },
};

const initialLocations: Record<string, Location> = {
  crimson_veil: {
    id: "crimson_veil",
    type: "LOCATION",
    displayName: "The Crimson Veil",
    shortDescription: "A sagging timber-frame structure in the lower district.",
    longDescription:
      "The air inside is thick with the smoke of tallow candles and cheap hashish. The walls are draped in moth-eaten tapestries that attempt to hide the rot. It is a place where secrets are bought for coppers and dignity is the first thing checked at the door.",
    attributes: {
      Atmosphere: "Oppressive",
      Patronage: "Desperate",
    },
  },
};

const initialCharacters: Record<string, Character> = {
  madam_vespera: {
    id: "madam_vespera",
    type: "CHARACTER",
    displayName: "Madam Vespera",
    shortDescription: "The iron-willed matron of the Crimson Veil.",
    longDescription:
      "Vespera is a woman of indeterminate age, her face a map of hard decisions and cold winters. She wears a dress of faded brocade and carries a heavy ring of keys that jingles with every rhythmic step. Her eyes see not your face, but the weight of your coin purse.",
    stats: {
      authority: 7,
      logic: 4,
      volition: 6,
    },
    opinions: {
      YOU: "A stray dog with better boots than most. Potentially useful, likely trouble.",
    },
    attributes: {
      Status: "Matron",
      Affiliation: "The Veiled",
    },
  },
};

export function seedDatabase() {
  const count = db.prepare("SELECT COUNT(*) as count FROM entities").get() as { count: number };
  if (count.count === 0) {
    const insert = db.prepare(
      "INSERT INTO entities (id, type, displayName, shortDescription, longDescription, attributes, stats, opinions) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    const insertEntity = (entity: any) => {
      insert.run(
        entity.id,
        entity.type,
        entity.displayName,
        entity.shortDescription,
        entity.longDescription,
        JSON.stringify(entity.attributes || {}),
        entity.type === "CHARACTER" ? JSON.stringify(entity.stats || {}) : null,
        entity.type === "CHARACTER" ? JSON.stringify(entity.opinions || {}) : null,
      );
      console.log(`Inserted ${entity.displayName}.`);
    };

    Object.values(initialObjects).forEach(insertEntity);
    Object.values(initialLocations).forEach(insertEntity);
    Object.values(initialCharacters).forEach(insertEntity);

    // Seed the root plot with two branch options
    db.prepare(
      `INSERT INTO plots (id, title, description, status, involved_locations, involved_characters, parent_plot_id, parent_option_id, child_plots)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      "plot_1",
      "The Missing Duke",
      "The old Duke was supposedly dead, but rumors spread that a man matching his description was seen near the Crimson Veil.",
      "PENDING",
      JSON.stringify(["crimson_veil"]),
      JSON.stringify(["madam_vespera"]),
      null,
      null,
      JSON.stringify([
        { plotId: null, triggerCondition: "Player asks Madam Vespera about the rusted coin" },
        { plotId: null, triggerCondition: "Player leaves the Crimson Veil without asking" },
      ]),
    );
    console.log("Seeded initial plot: The Missing Duke.");
  }
}

export function getAllEntities(): WorldState {
  const rows = db.prepare("SELECT * FROM entities").all() as any[];
  const state: WorldState = {
    objects: {},
    locations: {},
    characters: {},
  };

  rows.forEach((row) => {
    const entity = {
      id: row.id,
      type: row.type,
      displayName: row.displayName,
      shortDescription: row.shortDescription,
      longDescription: row.longDescription,
      attributes: JSON.parse(row.attributes),
      stats: row.stats ? JSON.parse(row.stats) : undefined,
      opinions: row.opinions ? JSON.parse(row.opinions) : undefined,
    };

    if (entity.type === "OBJECT") state.objects[entity.id] = entity as WorldObject;
    else if (entity.type === "LOCATION") state.locations[entity.id] = entity as Location;
    else if (entity.type === "CHARACTER") state.characters[entity.id] = entity as Character;
  });

  return state;
}

export function updateEntity(entity: Partial<WorldEntity> & { id: string }) {
  const existing = db.prepare("SELECT * FROM entities WHERE id = ?").get(entity.id) as any;
  if (!existing) return;

  const currentAttrs = JSON.parse(existing.attributes);
  const currentStats = existing.stats ? JSON.parse(existing.stats) : {};
  const currentOpinions = existing.opinions ? JSON.parse(existing.opinions) : {};

  // For update, we merge properties
  const newAttrs = entity.attributes ? { ...currentAttrs, ...entity.attributes } : currentAttrs;
  let newStats = currentStats;
  let newOpinions = currentOpinions;

  if (existing.type === "CHARACTER" && (entity as any).stats) {
    newStats = { ...currentStats, ...(entity as any).stats };
  }
  if (existing.type === "CHARACTER" && (entity as any).opinions) {
    newOpinions = { ...currentOpinions, ...(entity as any).opinions };
  }

  db.prepare(
    `
    UPDATE entities SET 
      displayName = COALESCE(?, displayName),
      shortDescription = COALESCE(?, shortDescription),
      longDescription = COALESCE(?, longDescription),
      attributes = ?,
      stats = ?,
      opinions = ?
    WHERE id = ?
  `,
  ).run(
    entity.displayName || null,
    entity.shortDescription || null,
    entity.longDescription || null,
    JSON.stringify(newAttrs),
    existing.type === "CHARACTER" ? JSON.stringify(newStats) : null,
    existing.type === "CHARACTER" ? JSON.stringify(newOpinions) : null,
    entity.id,
  );
  console.log(`Updated ${entity.displayName}.`);
}

export function upsertEntity(entity: WorldEntity) {
  db.prepare(
    `
    INSERT INTO entities (id, type, displayName, shortDescription, longDescription, attributes, stats, opinions)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      type = excluded.type,
      displayName = excluded.displayName,
      shortDescription = excluded.shortDescription,
      longDescription = excluded.longDescription,
      attributes = excluded.attributes,
      stats = excluded.stats,
      opinions = excluded.opinions
  `,
  ).run(
    entity.id,
    entity.type,
    entity.displayName,
    entity.shortDescription,
    entity.longDescription,
    JSON.stringify(entity.attributes || {}),
    entity.type === "CHARACTER" ? JSON.stringify((entity as any).stats || {}) : null,
    entity.type === "CHARACTER" ? JSON.stringify((entity as any).opinions || {}) : null,
  );
}

export function getAllEntitySummaries(
  typeFilter?: EntityType,
): { id: string; displayName: string; type: EntityType; shortDescription: string }[] {
  const rows = typeFilter
    ? (db.prepare("SELECT id, type, displayName, shortDescription FROM entities WHERE type = ?").all(typeFilter) as any[])
    : (db.prepare("SELECT id, type, displayName, shortDescription FROM entities").all() as any[]);
  return rows.map((r) => ({
    id: r.id,
    displayName: r.displayName,
    type: r.type as EntityType,
    shortDescription: r.shortDescription,
  }));
}

export function getEntityById(id: string): WorldEntity | null {
  const row = db.prepare("SELECT * FROM entities WHERE id = ?").get(id) as any;
  if (!row) return null;
  return {
    id: row.id,
    type: row.type,
    displayName: row.displayName,
    shortDescription: row.shortDescription,
    longDescription: row.longDescription,
    attributes: JSON.parse(row.attributes),
    ...(row.type === "CHARACTER" && {
      stats: JSON.parse(row.stats ?? "{}"),
      opinions: JSON.parse(row.opinions ?? "{}"),
    }),
  } as WorldEntity;
}

export function searchEntities(query: string): WorldEntity[] {
  const lower = query.toLowerCase();
  const rows = db.prepare("SELECT * FROM entities").all() as any[];
  return rows
    .filter(
      (r) =>
        r.displayName.toLowerCase().includes(lower) ||
        r.shortDescription.toLowerCase().includes(lower),
    )
    .slice(0, 5)
    .map((row) => ({
      id: row.id,
      type: row.type,
      displayName: row.displayName,
      shortDescription: row.shortDescription,
      longDescription: row.longDescription,
      attributes: JSON.parse(row.attributes),
      ...(row.type === "CHARACTER" && {
        stats: JSON.parse(row.stats ?? "{}"),
        opinions: JSON.parse(row.opinions ?? "{}"),
      }),
    })) as WorldEntity[];
}
