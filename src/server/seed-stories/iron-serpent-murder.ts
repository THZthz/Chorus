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

import type { SeedStory } from "@/server/seed-stories/types";
import { PLAYER_ID } from "@/shared/constants";

const objects: SeedStory["objects"] = {
  ornate_dagger: {
    id: "ornate_dagger",
    type: "OBJECT",
    displayName: "Ornate Dagger",
    shortDescription: "A ceremonial blade with a serpentine hilt, its edge still wet with blood.",
    longDescription:
      "The dagger is beautiful in a way that makes your skin crawl. The hilt is wrought in the shape of a coiled serpent, its scales rendered in rose gold, its eyes two chips of garnet that catch the light with a predatory gleam. The blade is slender, double-edged, engraved with a spiraling pattern that draws the eye down to the point — where a smear of blood, still tacky, clings to the steel.\n\nThis is not a weapon of convenience. It wasn't grabbed in a moment of passion. Someone chose this blade — ceremonial, distinctive, almost theatrical. The serpent motif is stamped with a maker's mark on the pommel: a tiny gear encircled by thorns. It belongs to someone with money and taste. Someone on this train.",
    attributes: {
      "Maker's Mark": "A tiny gear encircled by thorns — a Karavelle guild mark",
      Condition: "Recently used — blood still tacky, no oxidation",
      Significance: "Ceremonial, not practical — chosen deliberately, not grabbed in haste",
      Found: "Embedded in the victim's chest in Carriage 3",
    },
  },
  cryptic_telegram: {
    id: "cryptic_telegram",
    type: "OBJECT",
    displayName: "Folded Telegram",
    shortDescription:
      "A crumpled telegraph slip with a message that makes no sense — unless it does.",
    longDescription:
      "The paper is cheap telegraph stock, the kind you'd pick up at any relay station. But it's been folded and refolded so many times the creases are worn white. The message is brief, stamped in the mechanical type of an alchemical printer:\n\n  IRON SERPENT STOP CARRIAGE 3 STOP PACKAGE IS COMPROMISED STOP DO NOT LET IT REACH KARAVELLE STOP TRUST NO ONE STOP\n\nNo sender. No date. But the paper smells faintly of machine oil and ozone — the residue of aetheric transmission. Whoever sent this knew something was wrong before the train left the station. And whoever received it is now dead.",
    attributes: {
      Medium: "Alchemical telegraph — aetheric transmission residue present",
      Condition: "Heavily folded, creases worn white from repeated handling",
      "Key Phrases":
        "IRON SERPENT / CARRIAGE 3 / PACKAGE IS COMPROMISED / DO NOT LET IT REACH KARAVELLE",
      Missing: "No sender, no date — either redacted or deliberately omitted",
    },
  },
  silver_locket: {
    id: "silver_locket",
    type: "OBJECT",
    displayName: "Silver Locket",
    shortDescription:
      "A tarnished locket pried from the victim's hand, its portrait scratched beyond recognition.",
    longDescription:
      "The locket is old — the silver worn thin at the edges, the chain repaired twice with mismatched links. It was found clutched in the victim's left hand, the fingers frozen around it in rigor mortis. Someone had to break the thumb to get it free.\n\nInside, a miniature portrait has been deliberately scratched — gouged with the tip of a blade until the face beneath is nothing but a blur of scored paint and pale canvas. Whoever did this didn't want the victim identified. But they didn't throw the locket away. They left it in his hand. A message? A warning? Or sentiment — the killer couldn't destroy it entirely, even as they erased the face within.\n\nThe clasp is engraved with two initials, almost worn smooth: E.V.",
    attributes: {
      Material: "Tarnished silver, repaired chain with mismatched links",
      Portrait: "Deliberately defaced — gouged with a blade tip",
      "Initials on Clasp": "E.V. — nearly worn smooth",
      Found: "Clutched in the victim's left hand",
    },
  },
};

const locations: SeedStory["locations"] = {
  iron_serpent: {
    id: "iron_serpent",
    type: "LOCATION",
    displayName: "The Iron Serpent",
    shortDescription:
      "A steam-powered luxury express hurtling through the mountain passes — 200 souls, one killer.",
    longDescription:
      "The Iron Serpent is the pride of the Karavelle-Verdantis Railway — a steam-powered locomotive of gleaming brass and black iron, its engine a roaring heart of alchemical fire that drives it through the frozen mountain passes at impossible speed. Twelve carriages snake behind it: sleeping compartments paneled in dark mahogany, a dining car lit by crystal chandeliers that sway with the motion of the tracks, a lounge where the wealthy play cards and drink imported wine, and a baggage car where things less savory than luggage sometimes travel.\n\nThe train left Verdantis Station six hours ago. It is scheduled to reach Karavelle in another six. The blizzard outside has sealed the windows with frost; the mountain passes offer no stops, no escape. For the next six hours, the Iron Serpent is a closed world — a steel snake of steam and secrets, carrying a murderer who cannot leave.\n\nOutside, the wind howls. Inside, the chandeliers flicker. And in Carriage 3, the body of a man with no name lies cooling on the velvet floor.",
    attributes: {
      Route: "Verdantis to Karavelle — 12 hours through the Ironfang Mountains",
      "Current Status": "Six hours from Karavelle, blizzard conditions, no stops possible",
      "Train Composition": "12 carriages — sleepers, dining, lounge, baggage, crew quarters",
      Atmosphere: "Claustrophobic, luxurious, suspended — a sealed world in motion",
      "Notable Feature":
        "The engine runs on alchemical combustion — silent, powerful, and volatile if mishandled",
    },
  },
};

const characters: SeedStory["characters"] = {
  [PLAYER_ID]: {
    id: PLAYER_ID,
    type: "CHARACTER",
    displayName: "YOU",
    shortDescription:
      "An amnesiac on a luxury train with a violet crystal — witness, suspect, or something else entirely.",
    longDescription:
      "You remember nothing before the scream. The shriek of steam. The shudder of steel wheels on frozen track. A violet crystal pulsing in your clenched fist — and a woman's voice, somewhere in the carriage ahead, crying murder. You woke on the Iron Serpent, a luxury express hurtling through the frozen Ironfang Mountains, with no name, no ticket, and no idea how you got aboard. Now a man lies dead in Carriage 3, a detective watches everyone with suspicion, a noblewoman with a false identity was seen near the scene, the engineer is hiding cargo, and the conductor falsified the manifest. Six hours to Karavelle. Two hundred souls sealed in by a blizzard. One killer who cannot leave. And you — amnesiac, crystal-carrier, unknown to everyone including yourself — might be the key to the mystery, or its next victim.",
    stats: {
      logic: 4,
      rhetoric: 3,
      empathy: 4,
      perception: 5,
      volition: 4,
      endurance: 3,
      sorcery: 3,
      suggestion: 4,
      instinct: 5,
      might: 3,
      clockwork: 3,
      alchemy: 2,
    },
    opinions: {},
    conditions: {},
    attributes: {
      Amnesia: "Remembers nothing — woke on the train with no name or ticket",
      "Magical Affinity": "The violet crystal pulses with unknown energy",
      Status: "Unknown to everyone including himself — passenger, stowaway, or suspect",
    },
  },
  inspector_ashworth: {
    id: "inspector_ashworth",
    type: "CHARACTER",
    displayName: "Inspector Ashworth",
    shortDescription:
      "A detective of the Royal Investigative Service — too sharp for comfort, too curious for safety.",
    longDescription:
      "Ashworth is a man in his forties, lean and angular, with the kind of face that remembers everything and forgives nothing. Gray eyes, pale as winter slate, sit behind wire-rimmed spectacles that he polishes when he's thinking — which is constantly. He wears a traveling coat of dark wool, cut for function not fashion, and carries a pocket watch that he checks with mechanical regularity.\n\nHe joined the Royal Investigative Service young and rose fast — too fast, some said. His methods are unorthodox; his conviction rate is unmatched. He was on this train by accident, returning from a consultation in Verdantis, when the body was found. Now he's the only law on board — and he knows it. He has already sealed the crime scene, interviewed the staff, and begun building a list of suspects.\n\nThe problem is: everyone on this train has a secret. Including, perhaps, the inspector himself. He watches the player with a gaze that lingers a beat too long. He knows something. Or suspects something. Or maybe — just maybe — he's wondering if the amnesiac with the violet crystal is a witness, or a threat.",
    stats: {},
    opinions: {
      YOU: "No memory. No name. A crystal that glows when they're agitated. They're either the most dangerous person on this train or the most vulnerable. Either way, they're involved — the question is how. I've been doing this long enough to know when someone is at the center of a web. They are.",
      lady_marianne:
        "She's lying about who she is. The jewels are paste, the accent slips, and a woman of her supposed station doesn't travel without a maid. She was seen near Carriage 3 ten minutes before the body was discovered. But she's too clever to be this careless — unless she wants to be caught.",
    },
    conditions: {},
    attributes: {
      Occupation: "Detective Inspector, Royal Investigative Service",
      "Traveling From": "Verdantis — returning from a consultation",
      Reputation: "Unorthodox methods, unmatched conviction rate",
      "Known For": "Never closing a case without the truth — whatever the cost",
    },
  },
  lady_marianne: {
    id: "lady_marianne",
    type: "CHARACTER",
    displayName: "Lady Marianne",
    shortDescription: "A noblewoman with a viper's smile and a story that doesn't stay still.",
    longDescription:
      "She calls herself Lady Marianne d'Vere, widow of a Verdantine merchant prince, traveling to Karavelle to settle her late husband's affairs. The story is smooth as polished glass. Too smooth. The jewels at her throat are glass, not garnet. The accent drifts — one moment Verdantine refinement, the next something harder, forged in rougher places. And a woman of her supposed station does not travel without a maid, yet here she is, alone in a first-class compartment, remarkably unbothered by a murder one carriage over.\n\nShe is beautiful in a sharp, angular way — dark hair swept up with a single silver pin, cheekbones that could cut paper, a mouth that seems perpetually on the edge of a knowing smile. She wears black, ostensibly for mourning, but the cut is too fashionable, too deliberate. She ordered champagne at dinner. She laughed at something the conductor said. A woman whose husband recently died does not laugh like that.\n\nWhen interrogated, she is cooperative to the point of performance — answering questions before they're asked, volunteering details that seem helpful but lead nowhere. She is playing a game. The question is: what game, and who else is playing?",
    stats: {},
    opinions: {
      YOU: "The amnesiac. How romantic. How convenient. They're either a brilliant actor or the most unlucky soul on this train. Either way, they're in my way — unless I can make them useful. A person with no past is a person with nothing to lose. That's dangerous. That's valuable.",
      inspector_ashworth:
        "He sees too much. Those gray eyes miss nothing, and he's already circling. I need to give him a suspect before he starts looking too closely at me. But I can't point him at the wrong person too obviously — he'd smell the misdirection. He's not like the others. He's competent.",
    },
    conditions: {},
    attributes: {
      Occupation: "Alleged widow of a Verdantine merchant prince",
      "Traveling Under": "Lady Marianne d'Vere — almost certainly an alias",
      Tell: "Jewelry is paste, accent slips under pressure",
      Comportment: "Too calm, too cooperative, entirely too prepared",
    },
  },
  chief_engineer_kade: {
    id: "chief_engineer_kade",
    type: "CHARACTER",
    displayName: "Chief Engineer Kade",
    shortDescription:
      "The train's master of alchemical fire — grease-stained hands, clockwork heart, and a loyalty that runs deeper than steel.",
    longDescription:
      "Kade is built like the engine they tend — broad-shouldered, burn-scarred, perpetually humming with contained energy. Their forearms are a map of old burns and gear-grease tattoos; their right hand is partially clockwork — brass knuckles and delicate gears replacing three fingers lost to a steam accident years ago. They wear the railway uniform with the collar unbuttoned and the sleeves rolled to the elbow, as if formality itself is a heat they can't tolerate.\n\nKade has been with the Iron Serpent since her maiden voyage eight years ago. They know every bolt, every piston, every secret compartment in the locomotive's groaning heart. They know the schedule, the crew, the cargo manifests — and they know when something doesn't add up. The baggage car was sealed before departure on orders from the railway office. No manifest. No inspection. Kade didn't like it, but orders are orders.\n\nNow there's a body, a dagger with a gear-and-thorn mark Kade recognizes but won't explain, and a blizzard that's forcing the engine to burn hotter than safe limits. They're worried about the train. But they're also worried about something else — something in the baggage car that wasn't on any manifest.",
    stats: {},
    opinions: {
      YOU: "Another stray. This train collects them. They've got that look — the one people get when their past has been cut loose and they're drifting. I've seen it before, in the war. But that crystal they carry... I felt it the moment they walked past the engine room. The pressure gauges spiked. Something about them makes the alchemy unstable. I should keep them away from the boiler.",
      inspector_ashworth:
        "He's thorough. That's a problem. A thorough man will find the baggage car eventually, and then I'll have to explain why I didn't report the sealed cargo. I was following orders. That's not going to sound like enough, is it?",
    },
    conditions: {},
    attributes: {
      Occupation: "Chief Engineer, Iron Serpent — 8 years on this locomotive",
      "Partial Clockwork Hand": "Brass prosthetics replacing three fingers — fine motor control",
      Knows: "Every bolt, every secret, every unofficial cargo on the train",
      Concerned: "The baggage car was sealed on railway office orders — no manifest, no inspection",
    },
  },
  the_conductor: {
    id: "the_conductor",
    type: "CHARACTER",
    displayName: "Conductor Pell",
    shortDescription:
      "The train's chief of staff — impeccable uniform, fraying nerves, and a ledger full of irregularities.",
    longDescription:
      "Conductor Pell is a man in his late fifties, silver-haired and ramrod-straight in a uniform that has been pressed to a razor's edge. His mustache is waxed, his shoes are mirrors, his ticket punch gleams like a ceremonial weapon. He has worked the railway for thirty years and has never — never — had a murder on his train. He is taking it as a personal affront.\n\nBeneath the polish, Pell is fraying. His hands tremble when he isn't gripping something. He has checked the passenger manifest seven times since the body was discovered, as if a new name might appear that explains everything. The truth is simpler and worse: there are passengers on this train who are not on the manifest. There is cargo that was never inspected. And Pell looked the other way — a bribe, a favor, a debt called in — and now a man is dead and he can't tell the inspector what he knows without incriminating himself.\n\nHe wants to do the right thing. He always has. But the right thing has become expensive, and Pell has a daughter in Karavelle with medical bills and a railway pension that won't cover them.",
    stats: {},
    opinions: {
      YOU: "I don't know who they are. They're not on the manifest — I checked. I checked six times. But Inspector Ashworth hasn't detained them, so maybe... maybe it's fine. Maybe they're working with him? Or maybe I should tell him. But if I do, he'll start asking about the other names. The ones I added. The ones I removed.",
      chief_engineer_kade:
        "Kade knows about the baggage car. We haven't spoken about it — we don't need to. We've been on this route together for eight years; we know when to look the other way. But Kade's worried. That makes me worried. Kade doesn't worry about anything.",
    },
    conditions: {},
    attributes: {
      Occupation: "Conductor, Karavelle-Verdantis Railway — 30 years of service",
      "Current State": "Fraying — first murder on his train, and he knows more than he's saying",
      "Complicit In": "Falsified passenger manifest, uninspected cargo in the baggage car",
      Vulnerability: "A daughter in Karavelle, medical bills, a pension that won't cover them",
    },
  },
};

export const ironSerpentMurder: SeedStory = {
  id: "iron-serpent-murder",
  settingDescription:
    "The Iron Serpent, a steam-powered luxury express hurtling through the frozen Ironfang Mountains — twelve carriages of dark mahogany, crystal chandeliers, and alchemical fire. Six hours from Karavelle, a blizzard seals the train from the outside world. The player wakes to a scream in Carriage 3, a violet crystal pulsing in their palm, with no memory of who they are — or whether they are witness or suspect. A man is dead, a ceremonial dagger in his chest, a defaced locket in his hand, and a cryptic telegram warning of a compromised package that must not reach the city. Two hundred souls, no stops, no escape — and one killer who cannot leave.",
  toneDescription:
    "Tense, claustrophobic, noir-inflected. Rich sensory detail — steam and polished brass, the sway of chandeliers, the howl of wind against frosted windows, the weight of secrets in confined spaces. A closed-circle mystery where everyone has something to hide. Suspicion and seduction intertwined. Every conversation is a chess move; every alliance has a cost.",
  objects,
  locations,
  characters,
  rootPlot: {
    id: "plot_1",
    title: "Murder on the Iron Serpent",
    description:
      "The Iron Serpent thunders through the frozen Ironfang Mountains, six hours from Karavelle with no stops and no escape. In Carriage 3, a man lies dead — a ceremonial dagger in his chest, a defaced locket in his hand, and a cryptic telegram in his pocket warning of a compromised package that must not reach the city. The player woke to the scream that discovered the body, still clutching the violet crystal, still with no memory of who they are. Inspector Ashworth of the Royal Investigative Service has sealed the train and begun his investigation. Lady Marianne d'Vere, a noblewoman with a false identity, was seen near the crime scene. Chief Engineer Kade is hiding something in the baggage car. Conductor Pell falsified the passenger manifest. And somewhere on this train, a killer is waiting out the blizzard. With six hours until Karavelle, the player must navigate a web of secrets, survive the suspicions of a brilliant detective, and discover whether they are a witness to murder — or the architect of it.",
    status: "IN_PROGRESS",
    involvedLocations: ["iron_serpent"],
    involvedCharacters: [
      "inspector_ashworth",
      "lady_marianne",
      "chief_engineer_kade",
      "the_conductor",
    ],
    childPlots: [
      {
        plotId: null,
        triggerCondition:
          "Player aids Inspector Ashworth's investigation, gaining his trust while hiding their own secrets",
      },
      {
        plotId: null,
        triggerCondition:
          "Player forms an uneasy alliance with Lady Marianne, navigating her web of half-truths to uncover the bigger conspiracy",
      },
      {
        plotId: null,
        triggerCondition:
          "Player investigates the baggage car with Kade, discovering what the killer is really after — and what it has to do with the crystal",
      },
    ],
  },
  initialTime: { day: 1, segment: 11 },
  initialScene: {
    currentLocationId: "iron_serpent",
    characterLocations: {
      player: "iron_serpent",
      inspector_ashworth: "iron_serpent",
      lady_marianne: "iron_serpent",
      chief_engineer_kade: "iron_serpent",
      the_conductor: "iron_serpent",
    },
    objectPositions: {
      ornate_dagger: { type: "location", locationId: "iron_serpent" },
      cryptic_telegram: { type: "character", characterId: "inspector_ashworth" },
      silver_locket: { type: "character", characterId: "inspector_ashworth" },
    },
  },
};
