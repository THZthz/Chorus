# emberwatch.sw — Chorus² v1 acceptance sample: one heartbeat, one fire, six hours.
#
# A lone lighthouse keeper feeds the beacon until dawn.
# WIN  — hold beacon.heat >= 1 (beacon_lit) when dawn fires: the first night tick at
#        clock >= 6h selects dawn and, with the beacon lit, ends "survived".
# LOSE — dawn with heat 0 ends "gray_dawn"; dread >= 5 ends "despair", and broken
#        (priority 1) is scanned before dawn, so dread must stay under 5 until 6h.
# LOOP — the boot checkpoint (t=0) starts night and furnace. The warden starts at
#        hunger 2; pang adds +1 per hour. From hunger >= 3 (starving) each hour
#        either gnaw burns one restore card (and -1 heat) or, with no restore card
#        left in the world, dread_first adds +1 dread. sup.eat (morsel slots) is the
#        only hunger relief; dream draws calm (restore) from the omens deck to hold
#        dread off. Playing nothing, dread reaches 5 in hour 5 — before dawn.
# HEAT — blaze (+3, once) and feed (+1..+4 through coal and kindle slots) must
#        outrun the furnace's hourly -1; comb can wash driftwood ashore (into shore)
#        for feed's greedy open kindle slot to pull.
# Priority is ascending everywhere: in tend, handbook(5) beats blaze(10) beats
# feed(20), with routine(100) the last resort; in night, broken(1) beats dawn(2)
# beats dread_first(5) beats gnaw(10) beats pang(20).
#
# Core loop exercised: aspect registry + read defaults; the unified cards namespace
# with lifetime (tea 3h; decaying driftwood and portents); recipe-level slots
# (consuming fuel/morsel, greedy open kindle) and per-recipe durations (every
# recipe declares its own); guard (feed); derived predicates
# (beacon_lit, starving, can_fuel); arithmetic and OR-of-totals; a multi-instance
# verb (seaward, instances 3); a chain-only verb opened by start (dream ->
# omen_sight); roll; deck draw; query consume (blaze, gnaw, read_portent); legacy
# write/read (dawns_seen); three endings; zone scope (zone:tower).
# NOT exercised here — the M2 test matrix owns full v1 coverage (spec.md §6).

game "Emberwatch"
  seed 7              # content-fixed seed: boot must be called with the same seed
  turn 10m            # minimum advance unit; durations stay arbitrary minute values
  home tower

zones
  tower
  shore

aspects
  heat: 0
  coal: 0
  kindle: 0
  watch: 0
  hunger: 0
  dread: 0
  restore: 0
  portent: 0
  stamped: 0

predicates
  predicate beacon_lit    = beacon.heat >= 1
  predicate starving(p)   = p.hunger >= 3 and p.restore < 1
  predicate can_fuel(n)   = total(world, aspect:coal) + total(interior, aspect:coal) >= n

cards
  card warden
    kind person  unique
    aspects watch 1, hunger 2
    start in tower

  card beacon
    kind fixture  unique
    start in tower

  card coal
    kind matter  stackable
    aspects coal 1
    start in tower count 4
    desc "Black and glinting."

  card tea
    kind provision  stackable
    aspects restore 1, stamped 1
    lifetime 3h
    start in tower count 1
    desc "Still warm. It will not keep."

  card driftwood
    kind matter  stackable
    aspects kindle 1
    lifetime 2h

  card portent
    kind omen  stackable
    aspects portent 1, dread 1
    lifetime 2h

  card gull
    kind omen  stackable
    desc "A gull screams at nothing."

  card calm
    kind omen  stackable
    aspects restore 1

  card token
    kind treasure  stackable
    desc "A carved token of thanks from the water."

decks
  deck omens
    entries portent, portent, gull, calm   # repetition = weighting
    fallback calm

legacies
  dawns_seen: 0

endings
  ending survived "The Beacon Holds" grand
    text "Dawn finds the lamp trimmed and burning."
  ending gray_dawn "A Gray Dawn" somber
    text "The light died in the small hours."
  ending despair "The Long Watch" dark
    text "You do not remember leaving the rail."

verbs
  verb tend committable instances 1
    title "Tend the Beacon"

    recipe handbook
      priority 5  craftable
      duration 30m
      when legacy.dawns_seen >= 1 and beacon.heat == 0
      effects
        set beacon.heat += 1
        narrate "Hands that kept a dawn before remember the first stroke."

    recipe blaze
      priority 10  craftable
      duration 30m
      when can_fuel(3) and beacon.heat <= 3
      effects
        consume aspect:coal 3
        set beacon.heat += 3
        narrate "You shovel the store straight into the furnace."

    recipe feed
      priority 20  craftable
      duration 30m
      guard "The furnace already roars." when beacon.heat >= 6
      slots
        fuel: 1..2 consuming aspect:coal
        kindle: 0..2 greedy open aspect:kindle
      effects
        set beacon.heat += total(slot:fuel, aspect:coal) + total(slot:kindle, aspect:kindle)
        narrate "The furnace drinks. Heat reads {beacon.heat}."

    recipe routine
      craftable
      duration 30m
      effects
        narrate "You oil the assembly and watch the flame steadier."

  verb sup committable instances 1
    title "Sup in the Cabin"
    recipe eat
      craftable
      duration 10m
      slots
        morsel: 1..2 consuming aspect:restore
      effects
        set warden.hunger -= 2 * total(slot:morsel, aspect:restore)
        narrate "Warm tea, cold bread. The night loosens its grip."

  verb dream committable instances 1
    title "Sleep"
    recipe sleep
      craftable
      duration 1h
      effects
        draw from omens into interior
        roll 1d6 + warden.watch as sight
        if sight >= 5
          set warden.watch += 1
          narrate "You wake with a cipher's shape behind your eyes ({sight})."
        else
          narrate "Gull-cries, cold water, nothing you can keep ({sight})."
        start omen_sight

  verb seaward committable instances 3
    title "Signal Seaward"
    recipe flash
      craftable
      duration 20m
      effects
        roll 1d6 + warden.watch as reply
        if reply >= 5
          spawn token into interior
          narrate "A boat answers and sends up a token."
        else
          narrate "Only dark water answers."

  verb omen_sight uncommittable instances 1
    title "Second Sight"
    recipe read_portent
      chain-only
      duration 15m
      when count(world, aspect:portent) >= 1 or count(interior, aspect:portent) >= 1
      effects
        consume aspect:portent 1
        roll 1d6 as depth
        if depth >= 4
          set warden.watch += 1
          narrate "The portent unfolds into plain words. You learn."
        else
          set warden.dread += 1
          narrate "It unfolds into teeth."

  verb comb committable instances 1
    title "Comb the Shore"
    recipe walk
      craftable
      duration 40m
      when count(zone:tower, aspect:kindle) < 3
      effects
        roll 1d6 as tide
        if tide >= 3
          spawn driftwood count 1 into shore
          narrate "Driftwood, still wet. It will burn."
        else
          narrate "Shingle and cold foam; nothing tonight."

  verb furnace autonomous instances 1
    title "The Fire"
    recipe cool
      priority 10
      duration 1h
      when beacon.heat >= 1
      effects
        set beacon.heat -= 1
        narrate "The fire eats its hour."
    recipe dark_watch
      priority 20
      duration 1h
      when beacon.heat == 0
      effects
        narrate "The beacon stands dark; the sea sounds enormous."

  verb night autonomous instances 1
    title "The Night"
    recipe pang
      priority 20
      duration 1h
      when warden.hunger >= 1
      effects
        set warden.hunger += 1
        narrate "Hunger counts itself against your ribs."
    recipe gnaw
      priority 10
      duration 1h
      when starving(warden)
      effects
        consume aspect:restore 1
        set beacon.heat -= 1
        narrate "You eat standing up, watching the flame."
    recipe dread_first
      priority 5
      duration 1h
      when starving(warden) and count(world, aspect:restore) == 0
      effects
        set warden.dread += 1
        narrate "Nothing to eat. The hunger turns to dread."
    recipe dawn
      priority 2
      duration 1h
      when clock >= 6h
      effects
        if beacon_lit
          legacy dawns_seen += 1
          ending survived
        else
          ending gray_dawn
    recipe broken
      priority 1
      duration 1h
      when warden.dread >= 5
      effects
        legacy dawns_seen = 0
        ending despair
