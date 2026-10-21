# M2 matrix entry 19: include splicing — the root declares the header, zones,
# and verbs; the included file splices the cards, aspects, and legacy in at the
# include point. One namespace across both files.

game "IncRoot"
  seed host
  turn 1m
  home hall

zones
  hall

include "matrix_inc_cards.sw"

verbs
  verb use_included committable instances 1
    recipe tap
      duration 0
      slots
        gem: 1..1 aspect:worth
      effects
        set beacon.charge += total(slot:gem, aspect:worth)
        narrate "charged {beacon.charge}"
