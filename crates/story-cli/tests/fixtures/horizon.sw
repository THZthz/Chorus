# horizon.sw — the analysis-horizon contract fixture: the game declares its
# own envelope (`meta horizon "6h"`), and an authored deadline recipe makes
# the envelope real at runtime. Without the meta the flip verb loops
# forever; with it, every query is bounded by default.

game "Horizon"
  seed 3
  turn 30m
  home bench
  meta horizon "6h"

zones
  bench

cards
  card coin
    kind matter  stackable
    start in bench count 1

endings
  ending dawn "Time Is Up" somber
    text "The horizon arrives."

verbs
  verb flip committable instances 1
    title "Flip the Coin"
    recipe toss
      craftable
      duration 30m
      effects
        roll 1d6 as toss
        narrate "The coin spins ({toss})."

  verb clock autonomous instances 1
    title "The Horizon"
    recipe chime
      priority 5
      duration 0
      when clock >= 6h
      effects
        ending dawn
