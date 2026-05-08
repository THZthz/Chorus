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

const objects: SeedStory["objects"] = {
  obsidian_lens: {
    id: "obsidian_lens",
    type: "OBJECT",
    displayName: "Obsidian Lens",
    shortDescription:
      "A lens of volcanic glass that shows stars that shouldn't exist — cold to the touch, even near flame.",
    longDescription:
      "The lens is a perfect disc of obsidian, ground so thin at the center that light passes through it as a faint violet glow. It was found mounted in a telescope in the Athenaeum's highest tower — a telescope pointed not at any known constellation, but at a patch of empty sky between the stars. When you look through it, you don't see the heavens. You see something beneath them — a lattice of impossible geometry, shapes that squirm at the edge of vision, patterns that repeat in ways that hurt to follow.\n\nThe lens is cold. Not cool — cold, like lake ice in midwinter, even when held inches from a flame. The astronomers who used it reported dreams. Then nightmares. Then visions that didn't stop when they were awake. The last astronomer to study it carved equations into the walls of his cell for three days before gouging out his own eyes. The lens was locked in the vault. Someone has brought it out again.",
    attributes: {
      Material: "Volcanic obsidian — impossibly thin at center, violet light transmission",
      Temperature: "Permanently cold — defies thermodynamics",
      "Effect on Observers":
        "Dreams → nightmares → waking visions → irreversible psychological damage",
      History: "Last astronomer to study it gouged out his own eyes after three days",
      Shows: "Not stars — something beneath the visible cosmos",
    },
  },
  tome_fragment: {
    id: "tome_fragment",
    type: "OBJECT",
    displayName: "Tome Fragment",
    shortDescription:
      "A single page torn from a book that was burned three centuries ago — the ink still wet.",
    longDescription:
      "The page is vellum, not paper — scraped hide, old and yellowed but somehow supple, as if it has been handled recently and often. The text is written in a script that predates any known alphabet: characters that seem to crawl when you aren't looking directly at them, spiraling inward toward a central diagram that depicts — something. A shape. A being. A truth so large that the page can only hold a sliver of it.\n\nAlong the margins, in a more recent hand, someone has written annotations in High Karavellin: \"The convergence is not astronomical. It is biological. The stars are not aligning. They are waking up.\" The ink on the annotations is still faintly damp, as if written hours ago. But the page was torn from the Codex Umbrarum — a heretical text that was burned by the Magewardens three centuries ago. Every known copy was destroyed. Every known copy.\n\nThe fragment was found pinned to the Athenaeum's vault door with a shard of obsidian. Whoever left it there wanted someone to find it. Or wanted something to be released.",
    attributes: {
      Material: "Vellum — old yet supple, recently handled",
      "Original Text":
        "Pre-alphabetic script — characters appear to move when not observed directly",
      Marginalia: 'High Karavellin annotations: "The stars are not aligning. They are waking up."',
      Origin: "Codex Umbrarum — burned by the Magewardens 300 years ago, every copy destroyed",
      Found: "Pinned to the Athenaeum vault door with a shard of obsidian",
    },
  },
  star_chart: {
    id: "star_chart",
    type: "OBJECT",
    displayName: "Star Chart",
    shortDescription:
      "A celestial map that keeps changing — the constellations rearranging themselves toward a pattern that isn't finished yet.",
    longDescription:
      "At first glance, it's a standard astronomical chart — brass armillary rings, calibrated markings, the familiar constellations picked out in silver leaf on a field of midnight blue enamel. But when you look again, the pattern has shifted. And again. Cassiopeia has moved three degrees north. The Serpent's Tail has gained two stars that weren't there before. The entire chart is slowly, almost imperceptibly, rearranging itself — all the constellations drifting toward a single point in the sky that the chart refuses to name.\n\nThe mechanism that drives it is embedded in the backing: layers of alchemical alloy and aetheric filaments that pulse with a light that matches the crystal in your hand. The chart was built centuries ago by someone who knew this convergence was coming. It is less a map and more a countdown. At the current rate of drift, the constellations will complete their new pattern in roughly seventy-two hours. The chart's maker etched a single phrase into the brass housing, in letters so small you need the obsidian lens to read them: \"When the pattern completes, the sleeper beneath wakes.\"",
    attributes: {
      Construction: "Brass armillary rings, silver leaf constellations on midnight blue enamel",
      Mechanism: "Alchemical alloy and aetheric filaments — self-adjusting",
      "Current Behavior": "Constellations slowly drifting toward an unnamed convergence point",
      Countdown: "Pattern completes in ~72 hours at current rate of drift",
      Inscription: '"When the pattern completes, the sleeper beneath wakes."',
      Resonance: "The aetheric filaments pulse in rhythm with the player's crystal",
    },
  },
};

const locations: SeedStory["locations"] = {
  celestial_athenaeum: {
    id: "celestial_athenaeum",
    type: "LOCATION",
    displayName: "The Celestial Athenaeum",
    shortDescription:
      "A mountain-top observatory-library where forbidden knowledge is kept — and something beneath it is waking.",
    longDescription:
      "The Celestial Athenaeum perches on the summit of Mount Aethel, a black stone needle against the sky, accessible only by a funicular railway that hasn't carried a passenger in decades. Built by an order of astronomer-monks who vanished a century ago, the Athenaeum is part observatory, part library, part prison — its deepest vaults contain texts the Magewardens deemed too dangerous to burn and too valuable to destroy.\n\nThe main tower rises seven stories, each ringed with brass telescopes aimed at different sectors of the sky. The library spirals through the central core — floor-to-ceiling shelves of leather-bound astronomical records, alchemical treatises, and things that are not books but were shelved alongside them anyway. The vaults below are carved directly into the mountain, sealed with alchemical locks that predate the duke's law. The current custodian, Astronomer Lyra, has lived here alone for ten years — cataloguing stars, maintaining the instruments, and ignoring the whispers that rise from the vaults at night.\n\nThree days ago, the whispers became a voice. Two days ago, the star chart began to shift. Last night, someone — or something — left a page from a book that doesn't exist pinned to the vault door. The convergence is coming. The sleeper beneath stirs. And the Athenaeum, silent for a century, is gathering its breath.",
    attributes: {
      Location: "Summit of Mount Aethel — accessible only by funicular railway",
      "Built By": "An order of astronomer-monks who vanished a century ago",
      Architecture: "Seven-story black stone tower, brass telescopes, spiral library",
      Below: "Vaults carved into the mountain — sealed with pre-Warden alchemical locks",
      "Current State":
        "Silent for decades, now stirring — whispers, moving charts, impossible events",
      "The Vaults": "Contain texts the Magewardens deemed too dangerous to burn",
    },
  },
};

const characters: SeedStory["characters"] = {
  astronomer_lyra: {
    id: "astronomer_lyra",
    type: "CHARACTER",
    displayName: "Astronomer Lyra",
    shortDescription:
      "The Athenaeum's sole custodian — brilliant, isolated, and no longer certain what is real.",
    longDescription:
      "Lyra is thirty-four years old and has spent the last ten of them alone on a mountain. She came to the Athenaeum as a junior archivist, fresh from the Karavelle Academy, tasked with cataloguing the astronomical records. The senior staff left one by one — retirement, transfer, disappearance — until only Lyra remained. She told herself she preferred the solitude. The stars were company enough. The work was important.\n\nShe is sharp-featured and perpetually cold, wrapped in wool coats and fingerless gloves even indoors. Her dark hair is pulled back in a functional twist, streaked with premature gray at the temples. Her eyes are the color of weak tea and carry the particular exhaustion of someone who hasn't slept well in years. She paces when she talks. She annotates everything — margins of books, edges of charts, the backs of her own hands.\n\nThree days ago, the whispers from the vault became intelligible. They said her name. They said other things. She has not slept since. The arrival of the player — amnesiac, carrying a crystal that resonates with the star chart's pulse — is either a miracle or a catastrophe. Lyra has spent too long alone to tell the difference. But she knows one thing: the convergence is real, the countdown is accelerating, and she cannot stop it by herself.",
    stats: {
      logic: 7,
      rhetoric: 3,
      empathy: 2,
      perception: 7,
      volition: 5,
      endurance: 4,
      sorcery: 5,
      suggestion: 2,
      instinct: 6,
      might: 2,
      clockwork: 6,
      alchemy: 7,
    },
    opinions: {
      YOU: "They carry a crystal that pulses in time with the star chart. That's not coincidence — there are no coincidences left. They don't remember who they are, but their presence here, now, three days before the convergence... the universe doesn't arrange things this neatly without intention. Are they here to stop it, or to complete it? I don't know. I need them. That frightens me more than the whispers.",
      the_hermit:
        "He's been living in the lower vaults for years — years, and I never knew. Or maybe I did, and I chose not to. He speaks of the sleeper as if it's inevitable, as if resistance is vanity. He may be right. But he's also been alone in the dark for a very long time, and that kind of solitude bends things. People. Truths. I don't know if he's a prophet or a symptom.",
    },
    conditions: {},
    attributes: {
      Occupation: "Sole Custodian, Celestial Athenaeum — 10 years",
      Academy: "Karavelle Academy — astronomy and alchemical mechanics",
      "Current State": "Sleep-deprived, frightened, intellectually electrified",
      Knows: "Everything in the Athenaeum's catalogue — and some things not in it",
      "Has Heard": "The whispers from the vault — they know her name",
    },
  },
  archivist_elowen: {
    id: "archivist_elowen",
    type: "CHARACTER",
    displayName: "Archivist Elowen",
    shortDescription:
      "A reclusive keeper of forbidden texts — ink-stained fingers, a tremor in her voice, and knowledge she wishes she could un-learn.",
    longDescription:
      "Elowen came to the Athenaeum five years ago as an assistant archivist, sent by the Magewardens to inventory the vault contents. She was supposed to stay three months. She never left. Not because she was trapped — because she learned something in the vaults that made leaving impossible. There is a secret buried in the Codex Umbrarum, a truth about the nature of the stars, and once you know it, the sky looks different. You can see the patterns. You can feel the convergence approaching. And if you try to tell anyone who hasn't read the texts, the words won't come — your throat closes, your tongue numbs, the knowledge defends itself.\n\nShe is small, pale, perpetually hunched as if expecting a blow. Ink stains her fingers in layers — black, blue, the deep violet of the vault's restricted inks. She carries a ring of keys that jangle when she walks, though she's forgotten which key opens which lock. She speaks in fragments, trailing off mid-sentence when the words vanish from her throat. She wants to help. The knowledge won't let her. But she can write — the prohibition only affects speech — and she has been writing for five years, filling journal after journal with everything she's learned about the sleeper, the convergence, and what happens when it wakes.",
    stats: {
      logic: 6,
      rhetoric: 1,
      empathy: 5,
      perception: 6,
      volition: 3,
      endurance: 2,
      sorcery: 4,
      suggestion: 1,
      instinct: 7,
      might: 1,
      clockwork: 4,
      alchemy: 3,
    },
    opinions: {
      YOU: "The crystal. I felt it the moment you entered the Athenaeum — a resonance I haven't felt since I read the Codex. You're connected to the sleeper somehow. Not a threat — a key. Or perhaps both. The texts talk about a bearer, a vessel, someone who carries a fragment of the sleeper's power without knowing it. But the bearer has a choice. The texts are unclear on what happens if they choose wrong.",
      astronomer_lyra:
        "She's been here ten years and still thinks the universe is benevolent. I envy that. But her faith in reason, in science, in the idea that the stars follow rules — it's going to break when the convergence comes. I've seen what's in the vault. The rules were written by something that isn't bound by them.",
    },
    conditions: {},
    attributes: {
      Occupation: "Archivist, Celestial Athenaeum — 5 years (originally a 3-month assignment)",
      "Sent By": "The Magewardens — she was supposed to inventory the vaults",
      Condition: "Afflicted by a knowledge-curse — cannot speak forbidden truths, can write them",
      "Has Produced":
        "Journals filled with everything she's learned about the sleeper and the convergence",
      "The Curse": "Throat closes, tongue numbs when attempting to speak certain truths aloud",
    },
  },
  the_hermit: {
    id: "the_hermit",
    type: "CHARACTER",
    displayName: "The Hermit",
    shortDescription:
      "A figure in the vaults who hasn't seen the sun in twenty years — and no longer believes the sky is empty.",
    longDescription:
      "No one knows his name. If the Athenaeum's records ever held it, those pages have been eaten by time or deliberately removed. He has lived in the lower vaults for at least two decades, surviving on mushrooms that grow in the dark, water that seeps through the mountain, and a faith so absolute it has become indistinguishable from madness.\n\nHe is old — sixty, seventy, ageless in the way deep darkness is ageless. His skin is translucent from years without sun, his eyes clouded with cataracts that make them gleam silver in low light. He wears robes that were once the order's habit, now reduced to rags held together with twine and conviction. He does not walk so much as drift, appearing in rooms you were certain were empty, standing at the edge of your vision until you turn.\n\nThe Hermit was one of the original astronomer-monks. He was present when the order sealed the deepest vault and abandoned the Athenaeum. He chose to stay — not as a custodian, but as a worshipper. He believes the sleeper beneath the mountain is not a monster but a god, and the convergence is not an apocalypse but an awakening. He has been waiting twenty years for someone to arrive carrying a crystal that glows violet. He was beginning to think the prophecies were wrong. Then you walked in.\n\nHe is not hostile. He is something worse: helpful. He will answer any question, guide any exploration, open any lock. He wants you to understand. He wants you to see what he sees. He wants you to be ready — because when the sleeper wakes, it will need a vessel, and all signs point to you.",
    stats: {
      logic: 4,
      rhetoric: 7,
      empathy: 6,
      perception: 8,
      volition: 9,
      endurance: 5,
      sorcery: 6,
      suggestion: 7,
      instinct: 8,
      might: 2,
      clockwork: 1,
      alchemy: 4,
    },
    opinions: {
      YOU: "There. There. The bearer comes at last. I knew — I knew the prophecies were not metaphor, not the ravings of frightened men sealing their doom. The crystal. You carry it. You feel its pulse. That is the heartbeat of a god, child. The sleeper chose you before you were born. Before you forgot. Before the world made you small. Do not fear the convergence. Fear what happens if you refuse it.",
      archivist_elowen:
        "Poor child. She knows the truth — she has read it, written it, filled her journals with it — but she cannot speak it. The curse is not the sleeper's doing. It is the Magewardens' ward, a coward's lock on a door that was never theirs to seal. She could be free if she accepted the truth instead of fighting it. But she still believes knowledge is neutral. It isn't. Knowledge has a side. It always has.",
    },
    conditions: {},
    attributes: {
      Identity: "One of the original astronomer-monks — true name lost or erased",
      "Time in Vaults": "20+ years — has not seen sunlight in two decades",
      Belief: "The sleeper is a god; the convergence is an awakening, not an apocalypse",
      "Physical State": "Translucent skin, cataract-silver eyes, drifts rather than walks",
      Role: "Self-appointed prophet and guide — wants the player to embrace their role as vessel",
      "The Prophecy": "A bearer would come carrying a violet crystal, before the convergence",
    },
  },
};

export const celestialAthenaeum: SeedStory = {
  id: "celestial-athenaeum",
  objects,
  locations,
  characters,
  rootPlot: {
    id: "plot_1",
    title: "The Sleeper Beneath",
    description:
      "High on Mount Aethel, the Celestial Athenaeum has stood silent for a century — a repository of forbidden astronomical knowledge, tended by a single astronomer who has been alone too long. But something has changed. The star chart in the observatory dome is reconfiguring itself toward an unknown convergence. Whispers rise from the sealed vaults below — whispers that know the astronomer's name. A page from a book burned three centuries ago was found pinned to the vault door with obsidian, its ink still wet. And deep in the catacombs, a hermit who hasn't seen sunlight in twenty years believes that a god is waking — and that the player, carrying a crystal that resonates with the sleeper's pulse, is the vessel it will inhabit when it rises. With seventy-two hours until the pattern completes, the player must navigate a crumbling observatory, decipher forbidden texts, and confront an impossible choice: stop the convergence and destroy something ancient and beautiful, or embrace it and become something that is no longer human.",
    status: "IN_PROGRESS",
    involvedLocations: ["celestial_athenaeum"],
    involvedCharacters: ["astronomer_lyra", "archivist_elowen", "the_hermit"],
    childPlots: [
      {
        plotId: null,
        triggerCondition:
          "Player works with Astronomer Lyra to understand the star chart's message, using science and sorcery to decode the convergence",
      },
      {
        plotId: null,
        triggerCondition:
          "Player delves into the vaults with Archivist Elowen, piecing together forbidden knowledge from her journals and the texts that curse those who read them",
      },
      {
        plotId: null,
        triggerCondition:
          "Player follows the Hermit's guidance into the deepest catacombs, learning the sleeper's true nature and the price of becoming its vessel",
      },
    ],
  },
  initialTime: { day: 1, segment: 10 },
  initialScene: {
    currentLocationId: "celestial_athenaeum",
    characterLocations: {
      astronomer_lyra: "celestial_athenaeum",
      archivist_elowen: "celestial_athenaeum",
      the_hermit: "celestial_athenaeum",
    },
    objectPositions: {
      obsidian_lens: { type: "location", locationId: "celestial_athenaeum" },
      tome_fragment: { type: "location", locationId: "celestial_athenaeum" },
      star_chart: { type: "location", locationId: "celestial_athenaeum" },
    },
  },
};
