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
  crumpled_letter: {
    id: "crumpled_letter",
    type: "OBJECT",
    displayName: "Crumpled Letter",
    shortDescription: "A creased parchment sealed with black wax, tucked inside your coat.",
    longDescription:
      "The letter is written in a tight, elegant hand. The black wax seal bears an emblem you don't recognize — a tower pierced by a crescent moon. The ink is smudged in places, as if damp.\n\n_To whom it concerns — Lady Seraphine Vex was found at dawn in the fountain of the upper courtyard. Her throat had been opened from ear to ear. The duke has locked down the upper levels. No one leaves Karavelle until the killer is found. I trust you remember why you were summoned._\n\nIt is unsigned.",
    attributes: {
      Case: "Murder of Lady Seraphine Vex",
      "Seal Emblem": "Tower pierced by crescent moon",
      Signed: "No",
    },
  },
  gilded_key: {
    id: "gilded_key",
    type: "OBJECT",
    displayName: "Gilded Key",
    shortDescription: "A brass key with an ivory lotus fob, to room seven of the Gilded Lotus.",
    longDescription:
      "The key is warm from your pocket. The ivory fob is carved into a lotus flower, petals slightly yellowed with age. The number '7' is stamped into the brass bow. The teeth are worn — this key has seen use.",
    attributes: {
      Room: "7, The Gilded Lotus",
      Material: "Brass and carved ivory",
    },
  },
};

const initialLocations: Record<string, Location> = {
  the_gilded_lotus: {
    id: "the_gilded_lotus",
    type: "LOCATION",
    displayName: "The Gilded Lotus",
    shortDescription: "An upscale pleasure house in the upper levels of Karavelle.",
    longDescription:
      "The Gilded Lotus occupies the top three floors of a renovated merchant's palace, its facade draped in silk banners that snap in the salt breeze off the harbor. Inside, the air is thick with incense — sandalwood and something floral, heavy and sweet. Dim lamplight filters through crimson silk shades, casting the corridors in a warm, perpetual dusk. Velvet divans line the walls of the common lounge, where courtesans in various states of undress lounge and whisper. The floors are dark hardwood, oiled and gleaming, muffling footsteps. From behind closed doors come murmurs, laughter, and the rhythmic creak of bedframes. The madam runs the house with an iron fist wrapped in velvet — no violence, no theft, and absolute discretion for those who can pay.",
    attributes: {
      District: "Upper Levels — Velvet Row",
      Proprietor: "Madam Cressida",
      Atmosphere: "Incense-choked, warm, deliberately languid",
    },
  },
  matt_harbor_upper: {
    id: "matt_harbor_upper",
    type: "LOCATION",
    displayName: "Upper Karavelle",
    shortDescription: "White limestone terraces where the merchant princes and minor nobility keep their townhouses.",
    longDescription:
      "White limestone terraces climb the hillside above the harbor, lined with acacia trees and gas-lamps that burn with alchemical flame. The streets are swept clean, patrolled by the duke's watch in polished breastplates. At night, the upper levels glitter like a necklace against the dark. But even here, in the shadow of the duke's clock-tower, the smell of the lower city drifts up on the wind — brine, smoke, and something rotting.",
    attributes: {
      Patrolled: "Duke's Watch — frequent patrols",
      Architecture: "White limestone, wrought iron, alchemical gas-lamps",
      "Notable Feature": "The Duke's Spire, a clock-tower that chimes in strange intervals",
    },
  },
  matt_harbor_lower: {
    id: "matt_harbor_lower",
    type: "LOCATION",
    displayName: "The Warrens",
    shortDescription: "The sunken underbelly of Karavelle — slave markets, smoke-belching workshops, and the lawless docks.",
    longDescription:
      "The lower levels are a labyrinth of leaning tenements, smoke-belching workshops, and open-air markets where anything can be bought — spices, stolen goods, information, and people. The slave markets huddle near the docks: orcish stevedores and elven merchants haggle over beastfolk laborers while gaunt-eyed handlers prod their wares. The air is a fog of coal smoke, fish-gut, cheap tallow, and the ever-present brine of the harbor. The duke's watch rarely descends below the third tier. Down here, the Harbor Rats run things, and justice is measured in coin and blood.",
    attributes: {
      Atmosphere: "Choking, crowded, lawless",
      "Notable Locations": "Slave markets, fighting pits, the Sinking Dock tavern",
      "Ruled By": "The Harbor Rats syndicate",
    },
  },
};

const initialCharacters: Record<string, Character> = {
  veyla: {
    id: "veyla",
    type: "CHARACTER",
    displayName: "Veyla",
    shortDescription: "A sharp-eyed courtesan at the Gilded Lotus, nursing secrets beneath silk.",
    longDescription:
      "Veyla has the look of a woman who has learned to read people the way a thief reads a lock — patiently, quietly, looking for the weak spring. She is tall, with dark copper skin and close-cropped black hair. A faded scar runs from her left collarbone to her shoulder blade, half-hidden by the strap of her shift. She moves with a dancer's economy. Her voice is low, with a slight accent — from the southern isles, perhaps. She found you unconscious in your room this morning and has been watching you ever since. She believes you are an investigator hired to look into Lady Seraphine's murder. She needs you to be that person. But she does not trust the confusion in your eyes — and confusion gets people killed in Karavelle.",
    stats: {
      logic: 4,
      rhetoric: 5,
      empathy: 6,
      perception: 6,
      volition: 3,
      endurance: 3,
      sorcery: 1,
      suggestion: 5,
      instinct: 4,
      might: 2,
      clockwork: 1,
      alchemy: 2,
    },
    opinions: {
      YOU: "Says they're an investigator. Doesn't remember. But the letter in their pocket is real, and so is the danger. I need them to be who they claim — because if they're not, I've already said too much.",
      madam_cressida: "Cressida knows everything that happens under her roof. She's let me stay despite the trouble I'm bringing. That means she wants something from this too.",
    },
    attributes: {
      Occupation: "Courtesan, Gilded Lotus",
      Origin: "Southern Isles (she claims)",
      "Known Associates": "Lady Seraphine Vex (deceased) — they were seen together the night of the murder",
      Status: "Nervous, hiding something",
    },
  },
  madam_cressida: {
    id: "madam_cressida",
    type: "CHARACTER",
    displayName: "Madam Cressida",
    shortDescription: "Proprietor of the Gilded Lotus — silk and steel in equal measure.",
    longDescription:
      "Cressida is a woman in her fifties, handsome rather than beautiful, with silver-streaked auburn hair pinned up in an elaborate coil. She wears dresses that cost more than most in the lower levels earn in a year — deep velvets, embroidered silks, always high-collared and long-sleeved. She is never without a glass of fortified wine and never drunk. Her smile is a social instrument, deployed with precision. She has run the Gilded Lotus for twenty years, and in that time she has accumulated enough secrets to bring down half the noble houses in Karavelle. She does not trade in them — not directly. She simply knows. And knowledge, in a city of liars, is the only real currency.",
    stats: {
      logic: 5,
      rhetoric: 7,
      empathy: 4,
      perception: 7,
      volition: 6,
      endurance: 3,
      sorcery: 1,
      suggestion: 6,
      instinct: 3,
      might: 1,
      clockwork: 2,
      alchemy: 2,
    },
    opinions: {
      YOU: "A guest who arrived three nights ago, paid in advance, and collapsed before reaching their room. Veyla has taken an interest — which means they're either valuable or dangerous. Possibly both.",
      veyla: "A good girl, clever, wasted on the trade. She's involved in something above her station. I should cut her loose before the trouble spreads. I won't.",
    },
    attributes: {
      Occupation: "Proprietor, The Gilded Lotus",
      Tenure: "20 years",
      "Known For": "Discretion, information brokerage, impeccable taste",
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

    // Seed the root plot — the murder of Lady Seraphine Vex
    db.prepare(
      `INSERT INTO plots (id, title, description, status, involved_locations, involved_characters, parent_plot_id, parent_option_id, child_plots)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      "plot_1",
      "The Murder of Lady Seraphine Vex",
      "Three nights ago, the duke's daughter was found in the upper courtyard fountain, her throat cut ear to ear. The duke has locked down the upper levels — no one leaves Karavelle. The player arrived at the Gilded Lotus claiming to investigate, or so Veyla says. But the player remembers nothing before waking in a silk-draped bed, a crumpled letter in their pocket and a stranger's name on their lips. The truth lies somewhere in the layers of Karavelle — the gilded lies of the upper city and the raw commerce of the Warrens below.",
      "IN_PROGRESS",
      JSON.stringify(["the_gilded_lotus", "matt_harbor_upper"]),
      JSON.stringify(["veyla", "madam_cressida"]),
      null,
      null,
      JSON.stringify([
        { plotId: null, triggerCondition: "Player investigates Seraphine's death through the noble houses of the upper levels" },
        { plotId: null, triggerCondition: "Player descends into the Warrens, where slaves and secrets are traded freely" },
        { plotId: null, triggerCondition: "Player uncovers Veyla's true connection to the murder" },
      ]),
    );
    console.log("Seeded initial plot: The Murder of Lady Seraphine Vex.");

    // Set initial game time to dawn
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "game_time_day",
      "1",
    );
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "game_time_segment",
      "2",
    );

    // Set initial scene — player's room at the Gilded Lotus
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "current_scene",
      JSON.stringify({
        currentLocationId: "the_gilded_lotus",
        characterLocations: {
          veyla: "the_gilded_lotus",
          madam_cressida: "the_gilded_lotus",
        },
        objectPositions: {},
      }),
    );
  }
}

function rowToEntity(row: any): WorldEntity {
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

export function getAllEntities(): WorldState {
  const rows = db.prepare("SELECT * FROM entities").all() as any[];
  const state: WorldState = { objects: {}, locations: {}, characters: {} };

  for (const entity of rows.map(rowToEntity)) {
    if (entity.type === "OBJECT") state.objects[entity.id] = entity as WorldObject;
    else if (entity.type === "LOCATION") state.locations[entity.id] = entity as Location;
    else if (entity.type === "CHARACTER") state.characters[entity.id] = entity as Character;
  }

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
  return ids.filter((id) => entityMap.has(id)).map((id) => rowToEntity(entityMap.get(id)!));
}

export function getEntityById(id: string): WorldEntity | null {
  const row = db.prepare("SELECT * FROM entities WHERE id = ?").get(id) as any;
  return row ? rowToEntity(row) : null;
}

export function getEntitiesByText(query: string): WorldEntity[] {
  const lower = query.toLowerCase();
  const rows = db.prepare("SELECT * FROM entities").all() as any[];
  return rows
    .filter(
      (r) =>
        r.displayName.toLowerCase().includes(lower) ||
        r.shortDescription.toLowerCase().includes(lower),
    )
    .slice(0, 5)
    .map(rowToEntity);
}
