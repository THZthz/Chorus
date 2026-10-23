# tiny.sw — a minimal analysis fixture for the story-cli test suite: a gated
# tap that can never open (the matter count can never reach 99), a gamble
# whose roll decides a legacy, an ending that fires on that legacy, and a
# second ending that no rule ever produces.

game "Tiny"
  seed 5
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

cards
  card stone
    kind matter  stackable
    start in bench count 2

legacies
  luck: 0

endings
  ending lit "Lit" grand
    text "The charge holds."
  ending dark "Dark" somber
    text "It never took."

verbs
  verb tap committable instances 3
    recipe knock
      craftable
      duration 10m
      when count(world, kind:matter) >= 99
      effects
        narrate "The stone does not care."

  verb gamble committable instances 3
    recipe flip
      max 3
      craftable
      duration 0
      effects
        roll 1d6 as fate
        if fate >= 4
          legacy luck = 1
          narrate "Fate favors you ({fate})."
        else
          narrate "Nothing stirs ({fate})."

  verb seal committable instances 1
    recipe light
      craftable
      duration 0
      when legacy.luck >= 1
      effects
        ending lit
