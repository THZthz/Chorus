// Seed content + initial world — the spec's flagship examples as data (§6.4).
import { coreDictionary, type Dictionary } from './dict.js'
import type { ContentRaw } from './content.js'
import { addEntity, relationKey, newState, type State } from './types.js'

export function seedDictionary(): Dictionary {
  return coreDictionary()
}

export function seedContent(): ContentRaw {
  return {
    verbs: {
      READ: {
        slots: [
          { role: 'actors', min: 1, max: 2, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:text' },
          { role: 'at', min: 0, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [
          { id: 'read.magick_gate', when: 'object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2)', out: ['REFUSE("The letters swim like minnows.")'] },
          { id: 'read.parent_child', when: 'count(actors) = 2 AND rel(parent_child, actors)', out: ['SPAWN BONDING_MEMORY', 'NARRATE("{actors} read aloud; the child sleeps wiser.")'] },
          { id: 'read.unrelated_pair', when: 'count(actors) = 2', out: ['NARRATE("{actors} trade the book back and forth and lose the thread.")'] },
          { id: 'read.secret', when: 'object.aspect.secret >= 3', out: ['SPAWN CLUE', 'NARRATE("{actors} tease out a hidden thread.")'] },
          { out: ['NARRATE("{actors} read {object}.")'] },
        ],
      },
      TALK: {
        slots: [
          { role: 'actors', min: 1, max: 3, query: 'kind:character' },
          { role: 'about', min: 0, max: 1, query: 'kind:idea' },
          { role: 'at', min: 0, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [
          { id: 'talk.plan', when: 'about.aspect.secret >= 1', out: ['SPAWN PLAN_INFILTRATION'] },
          { out: ['NARRATE("{actors} talk.")'] },
        ],
      },
      TRAVEL_TO: {
        slots: [
          { role: 'actors', min: 1, max: 3, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [{ out: ['MOVE ACTORS INTO OBJECT', 'NARRATE("{actors} travel to {object}.")'] }],
      },
      USE_ON: {
        slots: [
          { role: 'actors', min: 1, max: 5, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:rite' },
          { role: 'with', min: 0, max: 2, query: 'kind:item' },
          { role: 'at', min: 0, max: 1, query: 'kind:place' },
        ],
        duration: 180,
        recipes: [
          { id: 'rite.lantern_gate', when: 'total(actors, aspect:lantern) < object.aspect.demand', shown: true, hint: 'The rite wants pooled lantern — it will starve.', out: ['REFUSE("The rite starves for light.")'] },
          { out: [
            'PROGRESS(1h, "The circle begins to hum against your palms.")',
            'PROGRESS(2h, "Light leaks between the chalk lines.")',
            'DESTROY OBJECT', 'DESTROY WITH', 'SPAWN MARK_OF_ASCENSION INTO at',
          ] },
        ],
      },
      WAIT: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} wait.")'] }],
      },
      WORK: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'at', min: 0, max: 1, query: 'kind:place' },
        ],
        duration: 120,
        recipes: [{ out: ['NARRATE("{actors} work at {at}.")'] }],
      },
      EXPLORE: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} explore {object}.")'] }],
      },
      DREAM: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'with', min: 0, max: 1, query: 'kind:idea' },
        ],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} dream.")'] }],
      },
      LOCATE_AT: {
        slots: [
          { role: 'actors', min: 1, max: 3, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:place' },
        ],
        duration: 0,
        recipes: [{ out: ['MOVE ACTORS INTO OBJECT'] }],
      },
      ACQUIRE: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1 },
        ],
        duration: 0,
        recipes: [{ out: ['NARRATE("{actors} acquire {object}.")'] }],
      },
    },
    triggers: [
      { id: 'dusk_falls', slot: 32, out: ['SPAWN DREAD', 'NARRATE("The lamps come on.")'] }, // 16:00
    ],
    prototypes: {
      CLUE_SECRET_HIDEOUT: { kind: 'idea', aspects: { secret: 1 } },
      PLAN_INFILTRATION: { kind: 'idea' },
      BONDING_MEMORY: { kind: 'idea' },
      MARK_OF_ASCENSION: { kind: 'idea' },
      DREAD: { kind: 'state' },
      CLUE: {
        kind: 'idea',
        derive: { nameFrom: 'object', aspects: { secret: { fromObjectAspect: 'secret', minus: 2 } } },
      },
    },
    templates: {
      READ: '{actors} read {object} at {at}.',
      TALK: '{actors} talk about {about} at {at}.',
      TRAVEL_TO: '{actors} travel to {object}.',
      USE_ON: '{actors} use {object} at {at}.',
      WAIT: '{actors} wait.',
      WORK: '{actors} work at {at}.',
      EXPLORE: '{actors} explore {object}.',
      DREAM: '{actors} dream.',
      LOCATE_AT: '{actors} are at {object}.',
      ACQUIRE: '{actors} acquire {object}.',
    },
  }
}

export function seedState(): State {
  const s = newState()
  const place = (id: string, name: string, container?: string) =>
    addEntity(s, { id, kind: 'place', name, aspects: {}, attrs: container ? { container } : {}, bornAction: null, deadAction: null })

  place('house', 'HOUSE')
  place('study', 'STUDY', 'house')
  s.entities.get('study')!.aspects['study'] = 1
  place('dining', 'DINING_HALL', 'house')
  place('villa', 'VILLA')
  place('greenhouse', 'GREENHOUSE', 'villa')

  const character = (id: string, name: string, aspects: Record<string, number>, container: string) =>
    addEntity(s, { id, kind: 'character', name, aspects, attrs: { container }, bornAction: null, deadAction: null })
  character('ana', 'ANA', { occult: 2, lantern: 1 }, 'house')
  character('bertha', 'BERTHA', {}, 'house')
  character('follower', 'MY_FOLLOWER', { lantern: 2 }, 'house')

  const text = (id: string, name: string, aspects: Record<string, number | Record<string, number>>, container: string) =>
    addEntity(s, { id, kind: 'text', name, aspects, attrs: { container }, bornAction: null, deadAction: null })
  text('forgetting', 'THE_FORGETTING', { secret: 4 }, 'house')
  text('pale', 'THE_PALE_LADY', { secret: 3, language: { latin: 2 } }, 'study')
  text('fairy', 'FAIRY_TALES', { tender: 1 }, 'house')
  text('magick', 'MAGICK_BOOK', { magick: 1 }, 'study')

  addEntity(s, { id: 'rite', kind: 'rite', name: 'RITE_OF_ASCENSION', aspects: { demand: 3 }, attrs: { container: 'house' }, bornAction: null, deadAction: null })
  addEntity(s, { id: 'chalice', kind: 'item', name: 'CHALICE', aspects: {}, attrs: { container: 'house' }, bornAction: null, deadAction: null })

  // a clue already in play — the TALK ABOUT topic
  addEntity(s, { id: 'clue_hideout', kind: 'idea', name: 'CLUE_SECRET_HIDEOUT', aspects: { secret: 1 }, attrs: { container: 'house' }, bornAction: null, deadAction: null })

  // ANA is BERTHA's parent — the seed world's one social edge
  s.relations.set(relationKey('ana', 'bertha', 'parent_child', true), {
    a: 'ana', b: 'bertha', type: 'parent_child', value: 1, bornAction: 0, deadAction: null,
  })
  return s
}
