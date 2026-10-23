# horizon-bad.sw — a horizon meta value that is not a duration spelling:
# the CLI warns and runs unbounded.

game "Horizon Bad"
  seed 3
  turn 30m
  home bench
  meta horizon "soon"

zones
  bench

endings
  ending dawn "Time Is Up" somber
    text "The horizon arrives."
