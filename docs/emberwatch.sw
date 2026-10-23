# emberwatch.sw — Chorus² v1 acceptance sample: one heartbeat, one fire, six hours.
#
# A lone lighthouse keeper feeds the beacon until dawn, keeps a gull company,
# and reads the water. The sample exercises the full v1 mechanism set; the
# header maps each mechanism to where it lives.
#
# WIN  — hold beacon.heat >= 1 (beacon_lit) when dawn fires: the first night
#        tick at clock >= 6h selects dawn and, with the beacon lit, ends
#        "survived" and keeps a dawn (dawns_seen; the cup counted in cups).
# LOSE — dawn with heat 0 ends "gray_dawn"; dread >= 5 ends "despair", and
#        broken (priority 1) is scanned before dawn, so dread must stay under
#        5 until 6h.
# LOOP — the boot checkpoint (t=0) starts night and furnace. The warden starts
#        at hunger 2; pang adds +1 per hour. From hunger >= 3 (starving) each
#        hour either gnaw burns one restore card (and -1 heat) or, with no
#        restore card left in the world, dread_first adds +1 dread. sup.eat
#        (morsel slots, declares restore — tea's completion book answers) is
#        the hunger relief; dream draws calm from the omens deck to hold dread
#        off (max four sleeps a night).
# HEAT — blaze (+3 through the stoke effectset) and feed (+1..+4 through coal
#        and kindle slots; the burn itself rides the consuming slots) must
#        outrun the furnace's hourly -1; comb washes driftwood ashore, cast
#        fishes flotsam (a driftwood mint alias) off the wrapping tides deck,
#        and lamp.dim trades lantern oil for a measured point of heat.
# TIES — the warden boots knowing the gull and carrying the lantern
#        (relations registry, boot edges). A lucky seaward flash relates the
#        token to the keeper as carries — stealing the lantern's exclusive
#        edge — and the despairing bell tolls the gull away (unrelate from an
#        event rule).
# MECHANISM MAP — zones nested (cabin in tower) and scoped (zone:tower in
#        comb's when, zone:shore in salvage's); aspect registry with read
#        defaults; predicates derived (beacon_lit), parameterized
#        (starving, can_fuel), relation-reading unary (fed_the_gull); cards
#        unique/stackable/group (flare), mint alias (flotsam mints as
#        driftwood), lifetime (tea, driftwood, portent), boot relates
#        (warden), on complete aspect book (tea), on expire book (portent),
#        meta annotations (game, zones, cards, endings); decks weighted with
#        fallback (omens) and wrap (tides); legacies (dawns_seen, cups,
#        omens_lost); verbs multi-instance (seaward, cast), releases (log),
#        uncommittable chain (omen_sight); recipes label, priority, max
#        (dream), compound duration (log.close), declares (sup), guard
#        (feed), allow shadow (routine after polish), consuming/greedy/open
#        slots and a predicate slot query (lamp.pour's oily()), effectset
#        calls with literal and expression-argument shapes (blaze); effects
#        spawn into zone (comb, cast, seaward), destroy (distress), consume,
#        swap (salvage), multi-target set (seaward's token stamping) and
#        unclamped set (lamp.dim), move into carrier (lamp.pour), legacy,
#        narrate with slot/roll/legacy/related/subject tokens, roll with
#        bonus, draw count into zone (cast), start via (dream), continue via
#        (log), signal (distress), relate/unrelate (seaward, feed_gull, the
#        bell), ending, if/else; trigger books at all four sites (recipe
#        complete in comb, card complete aspect book in tea, card expire book
#        in portent, and the storm_bell event rules with priority/once/when);
#        first() and name:/-kind: query terms (salvage, the bell, cast);
#        bound scope (sup); prose interpolation of slots, rolls, legacies,
#        relations, and subject.

game "Emberwatch"
  seed 7              # content-fixed seed: boot must be called with the same seed
  turn 10m            # minimum advance unit; durations stay arbitrary minute values
  home tower
  meta subtitle "a night in six hours"

zones
  tower
  cabin in tower      # the scope of zone:tower includes the cabin
  shore
    meta shows_fog "true"

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
  oil: 0

predicates
  predicate beacon_lit    = beacon.heat >= 1
  predicate starving(p)   = p.hunger >= 3 and p.restore < 1
  predicate can_fuel(n)   = total(table, aspect:coal) + total(interior, aspect:coal) >= n
  predicate oily(p)       = p.oil >= 1

relations
  carries exclusive        # directed: at most one bearer per carried thing
  bonded symmetric exclusive
  knows                    # directed, accumulating

cards
  card warden
    kind person  unique
    aspects watch 1, hunger 2
    start in tower
    relates knows gull
    relates carries lantern
    meta ui_icon "keeper"

  card beacon
    kind fixture  unique
    start in tower

  card lantern
    kind tool  unique
    aspects oil 0
    start in tower
    desc "Cold brass, a glass churn."

  card coal
    kind matter  stackable
    aspects coal 1
    start in tower count 4
    desc "Black and glinting."
    meta ui_icon "coal_lump"

  card oil
    kind matter  stackable
    aspects oil 1
    start in tower count 2

  card bread
    kind provision  stackable
    aspects restore 1
    start in tower count 2
    desc "Cold bread, honest weight."

  card tea
    kind provision  stackable
    aspects restore 1, stamped 1
    lifetime 3h
    start in tower count 1
    desc "Still warm. It will not keep."
    on complete restore
      rule saucer
        effects
          narrate "The cup goes back on the shelf warm ({subject})."
          legacy cups += 1

  card driftwood
    kind matter  stackable
    aspects kindle 1
    lifetime 2h

  card flotsam
    kind wreck  stackable
    mint driftwood        # every mint of flotsam materializes a driftwood body

  card portent
    kind omen  stackable
    aspects portent 1, dread 1
    lifetime 2h
    on expire
      rule unread
        effects
          narrate "A portent dries to salt, unread — and felt."
          legacy omens_lost += 1
          set warden.dread += 1

  card gull
    kind omen  stackable
    start in tower count 1
    desc "A gull screams at nothing."

  card calm
    kind omen  stackable
    aspects restore 1

  card token
    kind treasure  stackable
    desc "A carved token of thanks from the water."

  card flare
    kind signal  group signals
    desc "One flare to a night; the locker holds no second."
    meta ui_icon "flare"

  card ebb
    kind tide  stackable
    desc "The water draws back and mutters."

  card flood
    kind tide  stackable
    desc "The water arrives without knocking."

decks
  deck omens
    entries portent, portent, portent, gull, calm   # repetition = weighting
    fallback calm
  deck tides
    entries ebb, flood
    wrap                                   # the tide never runs dry

legacies
  dawns_seen: 0
  cups: 0
  omens_lost: 0

endings
  ending survived "The Beacon Holds" grand
    text "Dawn finds the lamp trimmed and burning."
    meta epilogue_art "dawn_gold"
  ending gray_dawn "A Gray Dawn" somber
    text "The light died in the small hours."
  ending despair "The Long Watch" dark
    text "You do not remember leaving the rail."

# An author-side abbreviation: blaze burns fuel into heat.
effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

on event storm_bell
  rule toll_panic priority 5 when warden.dread >= 3
    effects
      unrelate warden from name:gull as knows
      set warden.dread += 1
      narrate "The bell counts your hours. The gull deserts you."
  rule toll_once once
    effects
      narrate "The bell's first toll rolls out over the water."

verbs
  verb tend committable instances 1
    title "Tend the Beacon"
    meta ui_hint "the whole job, honestly"

    recipe handbook
      priority 5  craftable
      duration 30m
      when legacy.dawns_seen >= 1 and beacon.heat == 0 and alive(beacon) == 1
      effects
        set beacon.heat += 1
        narrate "Hands that kept a dawn before remember the first stroke."

    recipe blaze
      priority 10  craftable
      duration 30m
      when can_fuel(3) and beacon.heat <= 3
      effects
        stoke(3)
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
        narrate "The furnace drinks ({fuel}). Heat reads {beacon.heat}."

    recipe polish
      craftable
      duration 30m
      effects
        narrate "Brass first. The glass remembers your sleeve."

    recipe routine
      craftable
      allow shadow           # polish and routine are the same beat by design
      duration 30m
      effects
        narrate "You oil the assembly and watch the flame steadier."

  verb sup committable instances 1
    title "Sup in the Cabin"
    recipe eat
      craftable
      duration 10m
      declares restore       # tea's on complete restore book answers this
      slots
        morsel: 1..2 consuming aspect:restore
        cup: 0..1 open aspect:restore   # the cup survives the meal; its book fires
      effects
        set warden.hunger -= 2 * total(bound, aspect:restore)
        narrate "Warm tea, cold bread. The night loosens its grip."

  verb lamp committable instances 1
    title "Trim the Lamp"
    recipe refill
      label "Feed the lantern oil"
      craftable
      duration 20m
      when lantern.oil <= 1
      slots
        pour: 1..1 oily()    # a slot filtered by a derived predicate
      effects
        move slot:pour into lantern
        set lantern.oil += total(slot:pour, aspect:oil)
        narrate "The wick drinks. Oil reads {lantern.oil}."
    recipe dim
      priority 150  craftable
      duration 10m
      when lantern.oil >= 1
      effects
        set lantern.oil -= 1 unclamped
        set beacon.heat += 1
        narrate "A measured burn, clocked in the log."

  verb dream committable instances 1
    title "Sleep"
    recipe sleep
      max 4                  # four sleeps a night, then the pillow refuses
      craftable
      duration 1h
      effects
        draw from omens count 2 into interior
        roll 1d6 + warden.watch as sight
        if sight >= 5
          set warden.watch += 1
          narrate "You wake with a cipher's shape behind your eyes ({sight})."
        else
          narrate "Gull-cries, cold water, nothing you can keep ({sight})."
        start omen_sight via read_portent

  verb omen_sight uncommittable instances 1
    title "Second Sight"
    recipe read_portent
      chain-only
      duration 15m
      when count(anywhere, aspect:portent>=1) >= 1
      effects
        roll 1d6 as depth
        if depth >= 4
          consume aspect:portent 1
          set warden.watch += 1
          narrate "The portent unfolds into plain words. You learn."
        else
          set warden.dread += 1
          narrate "It unfolds into teeth, and stays."

  verb seaward committable instances 3
    title "Signal Seaward"
    recipe flash
      craftable
      duration 20m
      effects
        roll 1d6 + warden.watch as reply
        if reply >= 5
          spawn token into interior
          spawn flare into shore    # the group holds one: a second flash skips it
          set kind:treasure aspect:stamped += 1
          relate warden to kind:treasure as carries
          narrate "A boat answers: {related:carries of warden}. The lantern rides low, traded away."
        else
          narrate "Only dark water answers."

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
      on complete
        rule boots
          effects
            narrate "Boots salted, hands raw; the tower takes you back."

  verb cast committable instances 2
    title "Cast for Flotsam"
    recipe throw
      craftable
      duration 30m
      when count(anywhere, -kind:tide) >= 1
      effects
        draw from tides into shore
        spawn flotsam count 1 into shore
        narrate "The tide gives what it gives."

  verb salvage committable instances 1
    title "Salvage the Wreck Wood"
    recipe cut
      craftable
      duration 20m
      when count(zone:shore, name:flotsam) >= 1
      effects
        swap zone:shore name:flotsam to driftwood into shore
        narrate "Flotsam, split and stacked with the rest."

  verb distress committable instances 1
    title "Light the Distress Flare"
    recipe burn_flare
      craftable
      duration 10m
      when count(anywhere, name:flare) >= 1 and count(anywhere, -kind:tide) >= 1
      effects
        destroy kind:signal count 1
        signal storm_bell
        set warden.dread -= 1
        narrate "White fire, one breath long. The locker holds no second."

  verb feed_gull committable instances 1
    title "Share the Crust"
    recipe toss
      craftable
      duration 10m
      when rel(knows, warden, first(anywhere name:gull)) == 1
      effects
        relate warden to name:gull as bonded
        set warden.hunger += 1
        narrate "Bread for {related:bonded of warden}; your own stomach tighter for it."

  verb log committable instances 1
    title "Write the Log"
    releases cabin          # interior survivors land in the cabin, not home
    recipe write
      label "Set down the night"
      craftable
      duration 30m
      effects
        narrate "Ink in the cold: {legacy.dawns_seen} dawns kept, and counting."
        continue via close
    recipe close
      priority 200          # reached only through write's continue
      duration 1h30m        # a compound duration: ninety minutes of penmanship
      effects
        narrate "The log closes. The lamp wants trimming."

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
