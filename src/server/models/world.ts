import db from "@/server/db";
import {
  WorldEntity,
  WorldState,
  Character,
  Location,
  WorldObject,
  EntityType,
} from "@/types/entities";

const initialObjects: Record<string, WorldObject> = {
  brass_resonator: {
    id: "brass_resonator",
    type: "OBJECT",
    displayName: "Brass Ley-Resonator",
    shortDescription: "A humming device of black-iron gears and etched copper coils.",
    longDescription:
      "A fist-sized contraption of riveted brass and alchemically treated copper. Its innards click and whir, and a faint blue glow pulses from a cracked crystal at its core — a crude attempt to 'tune' ley-line energy through clockwork. The thing feels warm, almost alive, and gives off a faint smell of ozone and burnt herbs. It should not work. It does.",
    attributes: {
      Function: "Ley-resonance transduction (unstable)",
      Origin: "Unknown — possibly the Workshop of the Veiled Hand",
      "Steam Pressure": "Dangerously high",
    },
  },
  cogwheel_amulet: {
    id: "cogwheel_amulet",
    type: "OBJECT",
    displayName: "Cogwheel Amulet",
    shortDescription: "A brass cog on a leather string, etched with faded runes.",
    longDescription:
      "The cog is warm to the touch, its teeth worn smooth by fingers that have worried it for years. Faint runes — old magical script — have been etched into the brass and filled with a dark, tarnished silver. It is a symbol of the Clockwrights' Guild, worn by those who walk the line between the arcane and the mechanical.",
    attributes: {
      Affiliation: "Clockwrights' Guild",
      Material: "Brass, silver, leather",
      Age: "Several decades",
    },
  },
};

const initialLocations: Record<string, Location> = {
  rusted_cog: {
    id: "rusted_cog",
    type: "LOCATION",
    displayName: "The Rusted Cog",
    shortDescription: "A smoke-stained tavern clinging to the edge of the Steamward quarter.",
    longDescription:
      "The Cog is a low-ceilinged haunt of timber and blackened stone, sagging under the weight of its own years. A massive alchemical steam-boiler — installed by the late owner's son before he disappeared — hisses and clanks behind the bar, its copper pipes snaking across soot-stained walls like metallic ivy. The air is thick with pipe smoke, cheap tallow, and the bitter tang of boiled chicory. The clientele are a grim lot: discharged soldiers, hedge witches, failed alchemists, and the occasional steam-engineer nursing a grudge. A brass plaque above the hearth reads: 'The Old Ways End Here.'",
    attributes: {
      Atmosphere: "Smoke-choked and wary",
      District: "Steamward",
      "Notable Feature": "Installation of an alchemical steam-boiler behind the bar",
    },
  },
};

const initialCharacters: Record<string, Character> = {
  orin_fell: {
    id: "orin_fell",
    type: "CHARACTER",
    displayName: "Orin Fell",
    shortDescription: "The one-eyed keeper of the Rusted Cog, nursing old resentments.",
    longDescription:
      "Orin Fell has the look of a man who has been carved from bog-wood and left out in the rain. His one good eye — the other is a puckered scar hidden beneath a stained leather patch — misses nothing. He moves with a quiet, deliberate weight, wiping the same tankard with a rag that has long since given up on cleanliness. He despises the steam-boiler his son installed ('that infernal racket'), but he keeps it running because the ale-taps need the pressure. He knows everyone's name, everyone's debt, and everyone's secret. He just pretends not to.",
    stats: {
      authority: 6,
      perception: 7,
      volition: 5,
    },
    opinions: {
      YOU: "A new face with old eyes. Trouble walks behind him like a faithful hound.",
    },
    attributes: {
      Status: "Tavern Keeper",
      Affiliation: "None — despises all guilds equally",
      "Missing Eye": "Lost to a shrapnel burst during the Gutter-Wars",
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
      "The Engine That Should Not Be",
      "Strange things are happening in the Steamward quarter. A forgotten workshop in the old ward has begun to glow at night, and those who draw near hear the ticking of a thousand gears. Orin Fell's missing son may be connected — he was a clockwright's apprentice before he vanished. The air itself feels wrong, like magic is being drained from the leylines and fed into something metallic.",
      "PENDING",
      JSON.stringify(["rusted_cog"]),
      JSON.stringify(["orin_fell"]),
      null,
      null,
      JSON.stringify([
        { plotId: null, triggerCondition: "Player investigates the strange workshop in the old ward" },
        { plotId: null, triggerCondition: "Player presses Orin for answers about his missing son" },
      ]),
    );
    console.log("Seeded initial plot: The Engine That Should Not Be.");
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
    ? (db
        .prepare("SELECT id, type, displayName, shortDescription FROM entities WHERE type = ?")
        .all(typeFilter) as any[])
    : (db.prepare("SELECT id, type, displayName, shortDescription FROM entities").all() as any[]);
  return rows.map((r) => ({
    id: r.id,
    displayName: r.displayName,
    type: r.type as EntityType,
    shortDescription: r.shortDescription,
  }));
}

export function getEntitiesByIds(ids: string[]): WorldEntity[] {
  if (ids.length === 0) return [];
  const placeholders = ids.map(() => "?").join(", ");
  const rows = db
    .prepare(`SELECT * FROM entities WHERE id IN (${placeholders})`)
    .all(...ids) as any[];
  const entityMap = new Map(rows.map((row) => [row.id, row]));
  return ids
    .filter((id) => entityMap.has(id))
    .map((id) => {
      const row = entityMap.get(id)!;
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
    });
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
