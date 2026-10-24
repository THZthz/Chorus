# horizon-bad.sw — a horizon meta value that is not a duration spelling:
# the CLI warns and runs unbounded.

game "Horizon Bad"
  seed 3
  turn 30m
  home bench
  meta horizon "soon"

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

