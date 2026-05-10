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

import type { SeedStory } from "./types";
import { PLAYER_ID } from "@/shared/constants";

const objects: SeedStory["objects"] = {
  soul_shard: {
    id: "soul_shard",
    type: "OBJECT",
    displayName: "Soul Shard",
    shortDescription:
      "A thumbnail-sized crystal that pulses with a faint violet light, warm against your palm.",
    longDescription:
      "The crystal is smooth and oval, like a river stone worn down over centuries. It glows faintly — a deep violet luminescence that seems to breathe in rhythm with your heartbeat. When your emotions surge, it grows warmer, brighter. Veyla says you were clutching it when she found you, your fingers locked around it so tightly she had to pry them open one by one. The surface is unmarked, but when you stare into its depths, you see shapes — half-formed, like faces in smoke. It feels less like an object and more like a piece of something that was once whole.\n\nThe shard responds to desire, to fear, to longing. The first time your hand brushed Veyla's, it flared so bright it cast shadows on the walls.",
    attributes: {
      Origin: "Unknown — Veyla found it clutched in the player's hand",
      "Magical Resonance": "Pulses with the player's heartbeat; responds to strong emotion",
      Material: "Crystalline, warm to the touch, seemingly indestructible",
    },
  },
  veyllas_ribbon: {
    id: "veyllas_ribbon",
    type: "OBJECT",
    displayName: "Veyla's Ribbon",
    shortDescription:
      "A slip of midnight-blue silk tied around your wrist, carrying the scent of night-blooming jasmine.",
    longDescription:
      "The ribbon is soft and worn at the edges — something she's had for years. Midnight blue, the color of the sky just before dawn. She tied it around your wrist while you slept, a small ward against the things that hunt in dreams. The scent of night-blooming jasmine clings to the silk, mingled with something earthier — sandalwood and skin. When you hold it to your nose, you catch the faintest trace of something else beneath the perfume: ozone, the sharp tang of distant lightning. Veyla's own magic, perhaps, or a trace of whatever she touched before she touched you.",
    attributes: {
      Owner: "Veyla",
      "Scent Profile": "Night-blooming jasmine, sandalwood, ozone",
      Purpose: "A protective charm, tied while the player was unconscious",
    },
  },
};

const locations: SeedStory["locations"] = {
  the_velvet_thorn: {
    id: "the_velvet_thorn",
    type: "LOCATION",
    displayName: "The Velvet Thorn",
    shortDescription:
      "A brothel nestled deep in the Warrens — smoke, silk, and whispered promises.",
    longDescription:
      "The Velvet Thorn hides in plain sight on a crooked alley off the Sinking Dock, its entrance marked only by a single red lantern and a faded sign bearing a thorn-wrapped rose. Inside, the air is thick with the scent of clove smoke, spilled wine, and something sweeter underneath — jasmine oil, sweat, and candle wax. Low ceilings press down like a held breath. The main parlor is a warren of velvet-draped alcoves and worn leather divans, lit by guttering candles that leave most of the room in suggestive shadow. Upstairs, narrow corridors twist past numbered doors, each one muffling its own secrets. The floorboards creak in a language everyone ignores.\n\nThe clientele is a mix of dock workers spending their wages, merchants slumming it below their station, and the occasional Harbor Rat lieutenant conducting business in the back rooms. Madam Cressida keeps a strict rule: no blood on the sheets, no debts unpaid, and no questions about where a girl came from. The Thorn is not the finest brothel in Karavelle, but it is the most honest about what it sells — pleasure, escape, and the promise that for a few hours, no one is looking for you.",
    attributes: {
      District: "Lower Levels — The Warrens, off Sinking Dock",
      Proprietor: "Madam Cressida",
      Atmosphere: "Smoky, intimate, dangerous — velvet and candlelight over rough timber",
    },
  },
  matt_harbor_upper: {
    id: "matt_harbor_upper",
    type: "LOCATION",
    displayName: "Upper Karavelle",
    shortDescription:
      "White limestone terraces where the merchant princes and minor nobility keep their townhouses.",
    longDescription:
      "White limestone terraces climb the hillside above the harbor, lined with acacia trees and gas-lamps that burn with alchemical flame. The streets are swept clean, patrolled by the duke's watch in polished breastplates. At night, the upper levels glitter like a necklace against the dark. But even here, in the shadow of the duke's clock-tower, the smell of the lower city drifts up on the wind — brine, smoke, and something rotting. The mages' guild maintains a quiet presence here, and unlicensed sorcery is punished severely.",
    attributes: {
      Patrolled: "Duke's Watch — frequent patrols",
      Architecture: "White limestone, wrought iron, alchemical gas-lamps",
      "Notable Feature": "The Duke's Spire, a clock-tower that chimes in strange intervals",
      "Magic Regulation": "Unlicensed sorcery is a crime — the Magewardens watch closely",
    },
  },
  matt_harbor_lower: {
    id: "matt_harbor_lower",
    type: "LOCATION",
    displayName: "The Warrens",
    shortDescription:
      "The sunken underbelly of Karavelle — slave markets, smoke-belching workshops, and the lawless docks.",
    longDescription:
      "The lower levels are a labyrinth of leaning tenements, smoke-belching workshops, and open-air markets where anything can be bought — spices, stolen goods, information, and people. The slave markets huddle near the docks: orcish stevedores and elven merchants haggle over beastfolk laborers while gaunt-eyed handlers prod their wares. The air is a fog of coal smoke, fish-gut, cheap tallow, and the ever-present brine of the harbor. The duke's watch rarely descends below the third tier. Down here, the Harbor Rats run things, and justice is measured in coin and blood. In the Warrens, magic is not policed — it is either feared, exploited, or consumed. A person with power who doesn't know how to wield it is not a person for long.",
    attributes: {
      Atmosphere: "Choking, crowded, lawless",
      "Notable Locations":
        "Slave markets, fighting pits, the Sinking Dock tavern, the Velvet Thorn brothel",
      "Ruled By": "The Harbor Rats syndicate",
      "Magic Underground": "Unregulated — mages hide here, or rule here",
    },
  },
};

const characters: SeedStory["characters"] = {
  [PLAYER_ID]: {
    id: PLAYER_ID,
    type: "CHARACTER",
    displayName: "YOU",
    shortDescription:
      "An amnesiac with a glowing crystal and a secret power, waking in the Velvet Thorn brothel deep in the Warrens.",
    longDescription:
      "You remember nothing before the rain. Cold cobblestones. The distant clang of a harbor bell. A woman's voice, low and urgent, calling you back from somewhere dark. Then the warm glow of a violet crystal in your palm, pulsing in time with your heartbeat. You woke in a velvet-draped room at the Velvet Thorn, a brothel in the Warrens, with Veyla's golden eyes watching you and a name you don't recognize on her lips. The shard responds to your emotions — flaring bright when your pulse quickens, when Veyla draws close, when desire or fear or fury surge through you. You don't know what you are. You don't know where the power comes from. But in a city where unlicensed magic is a crime and the Warrens devour the weak, ignorance is a death sentence — and what burns between you and the half-elf who saved you might be the only truth worth trusting.",
    stats: {
      logic: 3,
      rhetoric: 2,
      empathy: 4,
      perception: 4,
      volition: 3,
      endurance: 3,
      sorcery: 6,
      suggestion: 5,
      instinct: 3,
      might: 2,
      clockwork: 2,
      alchemy: 2,
    },
    opinions: {},
    conditions: {},
    attributes: {
      Amnesia: "Remembers nothing before waking in the Warrens three nights ago",
      "Magical Affinity": "Unknown latent sorcery — the violet crystal responds to his emotions",
      Status: "Vulnerable, powerful, unaware — a secret waiting to be discovered",
    },
  },
  veyla: {
    id: "veyla",
    type: "CHARACTER",
    displayName: "Veyla",
    shortDescription:
      "A half-elf courtesan with eyes like burnished gold — equal parts savior and temptation.",
    longDescription:
      "Veyla is tall and languid, with the kind of beauty that makes people forget what they were about to say. Her dark copper skin gleams in candlelight; her hair is cropped short, black silk threaded with a single streak of silver at the left temple. Her ears taper to delicate points she usually hides beneath a wrap, and her eyes — large, luminous, the color of aged whiskey held up to flame — betray her half-elven blood more than she'd like. A faded scar traces from her left collarbone to her shoulder blade, a memento of something she won't discuss.\n\nShe moves like smoke — slow, deliberate, impossible to hold. Her voice is low and rough at the edges, salted with an accent from the southern isles. She smells of jasmine oil and something sharper underneath: ozone before a storm, the faint electric prickle of dormant magic she claims is too weak to matter.\n\nShe found you three nights ago, crumpled in the rain outside the Sinking Dock, a violet crystal clutched to your chest and no memory of your name. Something in that crystal sang to her blood. She dragged you through the Warrens herself — half-carried, half-coaxed — and paid Cressida for the room from her own purse. She has not left your side since. She tells herself it's because you need protection. But when your hand brushes hers, the shard flares, and she feels it too — a resonance that has nothing to do with magic and everything to do with hunger. She knows you are dangerous. She does not know whether she wants to save you or devour you. The truth, she suspects, is both.",
    stats: {},
    opinions: {
      YOU: "They don't remember who they are, but that crystal isn't ordinary and neither are they. When they look at me, the shard burns — and so does my skin. I should be careful. But I brought them here. I tied the ribbon. Whatever this is, I started it.",
      madam_cressida:
        "Cressida sees everything. She knows the crystal is magic. She knows I'm lying about where I found them. She's letting it happen — which means either she cares about me more than she lets on, or she's already calculated the profit in it.",
    },
    conditions: {},
    attributes: {
      Occupation: "Courtesan, The Velvet Thorn",
      Race: "Half-elf (southern isles heritage)",
      Origin: "Southern Isles — fled to Karavelle years ago",
      "Magical Affinity": "Dormant, weak — but the player's shard resonates with it",
      Status: "Protective, intrigued, increasingly entangled",
    },
  },
  madam_cressida: {
    id: "madam_cressida",
    type: "CHARACTER",
    displayName: "Madam Cressida",
    shortDescription:
      "Proprietor of the Velvet Thorn — a woman who has seen everything and forgotten nothing.",
    longDescription:
      "Cressida is a woman in her fifties, handsome in the way a well-worn blade is handsome — all function beneath the polish. Silver-streaked auburn hair is pinned in a coil that has seen better hours; she wears velvet in deep plum and emerald, dresses cut low but never cheap. A glass of fortified wine lives in her left hand like a prosthetic. She is never drunk, never shocked, and never without an angle.\n\nShe has run the Velvet Thorn for fifteen years. Before that, she ran a finer house in the upper levels — until a nobleman's indiscretion became a scandal and Cressida took the fall with a severance of scars across her back. She rebuilt in the Warrens, where the clientele is rougher but the truths are sharper. The Thorn is not luxurious, but it is safe — Cressida's rules are iron: no forced company, no unpaid debts, and no magic that draws the Magewardens' attention. The last rule is the one Veyla is currently testing.\n\nShe watches the player with the guarded patience of someone who has seen magic destroy lives before. She has not thrown them out — yet. Partly for Veyla's sake, partly because a man with no memory and a glowing crystal might be worth something to someone. The question is whether that someone is friend or buyer.",
    stats: {},
    opinions: {
      YOU: "Came in on Veyla's arm, three nights ago. Unconscious. No name, no coin, but that crystal in their hand would fetch a month's earnings from the right buyer. Veyla is besotted — or ensorcelled. Either way, trouble is brewing, and I need to decide whose side I'm on before it boils over.",
      veyla:
        "She's been with me two years. Clever, wounded, too good for this trade. That girl has been running from something since she stepped off the boat. Now she's running toward something — or someone. I hope she knows what she's doing. I hope I'm not the one who has to clean up the mess.",
    },
    conditions: {},
    attributes: {
      Occupation: "Proprietor, The Velvet Thorn",
      Tenure: "15 years at the Thorn; formerly ran a house in the upper levels",
      "Known For": "Iron rules, total discretion, a ledger of favors owed",
      Past: "Exiled from the upper levels after a noble scandal — rebuilt in the Warrens",
    },
  },
};

export const magicAwakening: SeedStory = {
  id: "magic-awakening",
  settingDescription:
    "Karavelle, the twin-faced port city of Matt Harbor. The upper levels gleam with white limestone and alchemical gas-lamps — the domain of merchant princes, minor nobility, and the duke's watch. Below, the Warrens fester in brine and coal-smoke, where slave markets trade in beastfolk and the Harbor Rats syndicate rules unchallenged. The player wakes in the Velvet Thorn, a brothel deep in the Warrens, with no memory, a violet crystal pulsing in their palm, and a beautiful half-elf named Veyla watching over them. Their latent sorcery is awakening — dangerous in a city where unlicensed magic is a crime. Veyla's own dormant elven blood resonates with the player's power, creating a pull between them that is equal parts magic and desire.",
  toneDescription:
    "Atmospheric, sensual, morally ambiguous. Rich sensory detail — smoke, silk, candlewax, jasmine, ozone, warm skin, old blood. Romance and danger intertwined. The world is tactile, intimate, and charged with unspoken wanting.",
  objects,
  locations,
  characters,
  rootPlot: {
    id: "plot_1",
    title: "The Awakening",
    description:
      "Three nights ago, the player was found unconscious in the rain outside the Sinking Dock, a violet crystal clutched in their hand and no memory of who they are. Veyla, a half-elf courtesan from the Velvet Thorn, dragged them to safety — something in the crystal sang to her dormant elven blood. Now the player wakes in a velvet-draped room, the shard pulsing with a light that responds to their every surge of emotion. They possess a latent sorcery they don't understand — powerful, untamed, and forbidden in the upper levels. Veyla is drawn to them with a pull that is equal parts magic and desire. But the Warrens are unforgiving, the Magewardens watch from above, and a person with power who doesn't know how to wield it is a prize, not a person. The truth of who the player is — and what they carry — lies somewhere between the velvet shadows of the Thorn, the glittering spires of the upper city, and the dangerous, undeniable heat between them and the half-elf who saved their life.",
    status: "IN_PROGRESS",
    involvedLocations: ["the_velvet_thorn", "matt_harbor_lower"],
    involvedCharacters: ["veyla", "madam_cressida"],
    childPlots: [
      {
        plotId: null,
        triggerCondition:
          "Player deepens their bond with Veyla and explores the nature of their magic through their connection",
      },
      {
        plotId: null,
        triggerCondition:
          "Player navigates the Warrens underworld — Harbor Rats, rogue mages, black markets — to find answers",
      },
      {
        plotId: null,
        triggerCondition:
          "Player seeks knowledge in the upper levels, risking exposure to the Magewardens and the duke's law",
      },
    ],
  },
  initialTime: { day: 1, segment: 1 },
  initialScene: {
    currentLocationId: "the_velvet_thorn",
    characterLocations: {
      player: "the_velvet_thorn",
      veyla: "the_velvet_thorn",
      madam_cressida: "the_velvet_thorn",
    },
    objectPositions: {
      soul_shard: { type: "character", characterId: PLAYER_ID },
      veyllas_ribbon: { type: "character", characterId: PLAYER_ID },
    },
  },
};
