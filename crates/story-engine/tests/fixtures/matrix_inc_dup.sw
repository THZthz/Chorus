# M2 matrix entry 19 (refusal path): a duplicate id across two included files
# is [E232], reported at the second declaration with both sites named.

game "IncDup"
  seed host
  turn 1m
  home hall

zones
  hall

include "matrix_inc_dup_a.sw"
include "matrix_inc_dup_b.sw"
