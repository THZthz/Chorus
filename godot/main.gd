extends Control

var bridge := ChorusBridge.new()
var repo_root := ""
var sw_file := "docs/emberwatch.sw"

var current_state: Dictionary = {}
var open_verb: String = "tend"
var current_page: int = 0
var dragging_eid: int = -1

# Staged slot bindings for the open verb window: Array of [slot_name, entity_id].
# An empty slot name asks the bridge to place the card.
var staged: Array = []

# Latest staging response per verb: verb_id -> staging view Dictionary
var staging_cache: Dictionary = {}

# Catalog of known entity data by entity_id: int -> Dictionary
var entity_catalog: Dictionary = {}

var table_canvas: TableCanvas
var situation_window: SituationWindow
var inspector_card: InspectorCard
var game_title_lbl: Label
var clock_lbl: Label
var legacy_lbl: Label
var undo_btn: Button
var verbs_flow: HFlowContainer
var zones_section: VBoxContainer
var _dragged_zone_card: CardView = null

func _ready() -> void:
	theme = UiKit.make_theme()
	repo_root = ProjectSettings.globalize_path("res://..").simplify_path()
	var demo_slot := false
	for arg in OS.get_cmdline_user_args():
		if arg.ends_with(".sw"):
			sw_file = arg
		elif arg == "--demo-slot":
			demo_slot = true

	_build_layout()
	var init_res := bridge.start(repo_root, sw_file)
	if not init_res.get("ok", false):
		_append_log("[color=#ff6b6b]Bridge error: %s[/color]" % init_res.get("error", "unknown"))
		return
	_append_log("[color=#8ce99a]Loaded %s (%s).[/color]" % [init_res.get("title", "Story"), sw_file])
	_apply_state(init_res)

	if demo_slot:
		stage("fuel", 4)
		on_card_clicked(4)

func _notification(what: int) -> void:
	if what == NOTIFICATION_PREDELETE or what == NOTIFICATION_WM_CLOSE_REQUEST:
		bridge.stop()
	elif what == NOTIFICATION_DRAG_END:
		if _dragged_zone_card != null:
			if _dragged_zone_card.get_parent() is ZoneCardFlow and not _dragged_zone_card.visible:
				var staged_set := _staged_set()
				if not staged_set.has(dragging_eid):
					_dragged_zone_card.get_parent().animate_card_returned(_dragged_zone_card)
			_dragged_zone_card = null
		if dragging_eid != -1:
			dragging_eid = -1
			_redraw_drop_targets()

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_ESCAPE:
			if situation_window.visible:
				close_window()
			elif inspector_card.visible:
				inspector_card.visible = false
		elif event.keycode == KEY_SPACE:
			_send({"cmd": "advance", "turns": 1})
		elif event.keycode == KEY_Z and (event.ctrl_pressed or event.meta_pressed):
			if not undo_btn.disabled:
				staged = []
				_send({"cmd": "undo"})
	elif event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		if situation_window.visible:
			var win_rect: Rect2 = situation_window.get_global_rect()
			if not win_rect.has_point(event.global_position):
				close_window()

class TableArea extends ScrollContainer:
	var ui: Node
	func _gui_input(event: InputEvent) -> void:
		if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
			if ui.situation_window.visible:
				var win_rect: Rect2 = ui.situation_window.get_global_rect()
				if not win_rect.has_point(event.global_position):
					ui.close_window()

	func _can_drop_data(_at: Vector2, data: Variant) -> bool:
		return typeof(data) == TYPE_DICTIONARY and data.get("type") == "card" \
			and not str(data.get("from_slot", "")).is_empty()

	func _drop_data(_at: Vector2, data: Variant) -> void:
		ui.unstage(int(data.get("entity_id", -1)))

class ZoneCardFlow extends HFlowContainer:
	func _init() -> void:
		add_theme_constant_override("h_separation", 10)
		add_theme_constant_override("v_separation", 10)

	func animate_card_removed(card_node: Control) -> void:
		if not is_inside_tree() or not card_node.visible:
			card_node.visible = false
			return

		var old_positions := {}
		for child in get_children():
			if child is Control and child.visible:
				old_positions[child] = child.global_position

		var fade_tween := card_node.create_tween().set_parallel(true)
		fade_tween.tween_property(card_node, "modulate:a", 0.0, 0.08)
		fade_tween.tween_property(card_node, "scale", Vector2(0.85, 0.85), 0.08)

		fade_tween.chain().tween_callback(func() -> void:
			card_node.visible = false
			card_node.modulate.a = 1.0
			card_node.scale = Vector2.ONE
			_apply_reflow(old_positions)
		)

	func animate_card_returned(card_node: Control) -> void:
		if not is_inside_tree() or card_node.visible:
			return

		var old_positions := {}
		for child in get_children():
			if child is Control and child.visible:
				old_positions[child] = child.global_position

		card_node.visible = true
		card_node.modulate.a = 0.0
		card_node.scale = Vector2(0.85, 0.85)

		await get_tree().process_frame

		for child in get_children():
			if child != card_node and child is Control and child.visible and old_positions.has(child):
				var target_pos: Vector2 = child.position
				var offset: Vector2 = old_positions[child] - child.global_position
				if offset.length() > 0.5:
					child.position += offset
					var tween := child.create_tween()
					tween.set_trans(Tween.TRANS_CUBIC).set_ease(Tween.EASE_OUT)
					tween.tween_property(child, "position", target_pos, 0.22)

		var pop_tween := card_node.create_tween().set_parallel(true)
		pop_tween.set_trans(Tween.TRANS_BACK).set_ease(Tween.EASE_OUT)
		pop_tween.tween_property(card_node, "modulate:a", 1.0, 0.18)
		pop_tween.tween_property(card_node, "scale", Vector2.ONE, 0.18)

	func _apply_reflow(old_positions: Dictionary) -> void:
		await get_tree().process_frame
		for child in get_children():
			if child is Control and child.visible and old_positions.has(child):
				var target_pos: Vector2 = child.position
				var offset: Vector2 = old_positions[child] - child.global_position
				if offset.length() > 0.5:
					child.position += offset
					var tween := child.create_tween()
					tween.set_trans(Tween.TRANS_CUBIC).set_ease(Tween.EASE_OUT)
					tween.tween_property(child, "position", target_pos, 0.22)

func _build_layout() -> void:
	# Full screen root
	set_anchors_preset(Control.PRESET_FULL_RECT)

	# 1. Felt Background & Canvas
	var bg := ColorRect.new()
	bg.color = UiKit.FELT
	bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(bg)

	table_canvas = TableCanvas.new()
	table_canvas.ui = self
	table_canvas.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(table_canvas)

	# 2. Main vertical layout: Top Felt Tabletop (Cards & Tokens) + Bottom Bar
	var root_vbox := VBoxContainer.new()
	root_vbox.set_anchors_preset(Control.PRESET_FULL_RECT)
	root_vbox.add_theme_constant_override("separation", 0)
	add_child(root_vbox)

	# Table play area scroll container
	var table_scroll := TableArea.new()
	table_scroll.ui = self
	table_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	table_scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	table_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	root_vbox.add_child(table_scroll)

	var table_content := MarginContainer.new()
	table_content.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	table_content.size_flags_vertical = Control.SIZE_EXPAND_FILL
	table_content.add_theme_constant_override("margin_left", 24)
	table_content.add_theme_constant_override("margin_right", 24)
	table_content.add_theme_constant_override("margin_top", 18)
	table_content.add_theme_constant_override("margin_bottom", 18)
	table_scroll.add_child(table_content)

	var table_vbox := VBoxContainer.new()
	table_vbox.name = "TableVBox"
	table_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	table_vbox.add_theme_constant_override("separation", 24)
	table_content.add_child(table_vbox)

	# Verbs row container
	var verbs_section := VBoxContainer.new()
	verbs_section.name = "VerbsSection"
	verbs_section.add_theme_constant_override("separation", 10)
	table_vbox.add_child(verbs_section)

	var verbs_hdr := UiKit.label("ACTIONS & SEASONS", 11, UiKit.TEXT_FAINT)
	verbs_section.add_child(verbs_hdr)

	var verbs_margin := MarginContainer.new()
	verbs_margin.add_theme_constant_override("margin_top", 18)
	verbs_section.add_child(verbs_margin)

	verbs_flow = HFlowContainer.new()
	verbs_flow.name = "VerbsFlow"
	verbs_flow.add_theme_constant_override("h_separation", 18)
	verbs_flow.add_theme_constant_override("v_separation", 24)
	verbs_margin.add_child(verbs_flow)

	# Zones section container
	zones_section = VBoxContainer.new()
	zones_section.name = "ZonesSection"
	zones_section.add_theme_constant_override("separation", 16)
	table_vbox.add_child(zones_section)

	# 3. Bottom Control Bar (Cultist Simulator style)
	var bar_panel := PanelContainer.new()
	bar_panel.custom_minimum_size.y = 52
	bar_panel.add_theme_stylebox_override(
		"panel",
		UiKit.box(UiKit.FELT_OUTER, Color(0, 0, 0, 0.8), 1, 0, 10)
	)
	root_vbox.add_child(bar_panel)

	var bar_hbox := HBoxContainer.new()
	bar_hbox.add_theme_constant_override("separation", 20)
	bar_panel.add_child(bar_hbox)

	game_title_lbl = UiKit.label("CHORUS²", 15, UiKit.WINDOW_TITLE)
	bar_hbox.add_child(game_title_lbl)

	legacy_lbl = UiKit.label("LEGACY: -", 12, UiKit.TEXT)
	legacy_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	bar_hbox.add_child(legacy_lbl)

	clock_lbl = UiKit.label("0m", 13, UiKit.HILITE)
	bar_hbox.add_child(clock_lbl)

	var adv1 := Button.new()
	adv1.text = "[ ADVANCE +1 ]"
	adv1.pressed.connect(func() -> void: _send({"cmd": "advance", "turns": 1}))
	bar_hbox.add_child(adv1)

	var adv6 := Button.new()
	adv6.text = "[ +6 TURNS ]"
	adv6.pressed.connect(func() -> void: _send({"cmd": "advance", "turns": 6}))
	bar_hbox.add_child(adv6)

	undo_btn = Button.new()
	undo_btn.text = "[ UNDO ]"
	undo_btn.disabled = true
	undo_btn.pressed.connect(func() -> void:
		staged = []
		_send({"cmd": "undo"})
	)
	bar_hbox.add_child(undo_btn)

	# Floating Overlays Layer (Situation Window & Top-Right Card Inspector)
	var overlay_layer := Control.new()
	overlay_layer.name = "OverlayLayer"
	overlay_layer.set_anchors_preset(Control.PRESET_FULL_RECT)
	overlay_layer.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(overlay_layer)

	# Top Right Inspector Card
	inspector_card = InspectorCard.new()
	inspector_card.visible = false
	inspector_card.position = Vector2(1060, 20)
	overlay_layer.add_child(inspector_card)

	# Centered Floating Situation Window
	situation_window = SituationWindow.new()
	situation_window.visible = false
	situation_window.position = Vector2(340, 120)
	overlay_layer.add_child(situation_window)

func _send(req: Dictionary) -> Dictionary:
	var res := bridge.request(req)
	if not res.get("ok", false):
		_append_log("[color=#ff6b6b]Refused: %s[/color]" % res.get("error", "Unknown error"))
		return res
	_apply_state(res)
	return res

func _apply_state(res: Dictionary) -> void:
	current_state = res
	undo_btn.disabled = not bool(res.get("can_undo", false))

	var title_str: String = str(res.get("title", "")).strip_edges()
	if not title_str.is_empty() and game_title_lbl != null:
		game_title_lbl.text = title_str.to_upper()

	var board: Dictionary = res.get("board", {})
	var c := int(board.get("clock", 0))
	var t := int(res.get("turn", 10))
	clock_lbl.text = "TIME: %dm (Turn: %dm)" % [c, t]

	var legacy_parts := PackedStringArray()
	var leg: Dictionary = board.get("legacy", {})
	for k in leg.keys():
		legacy_parts.append("%s: %d" % [k.to_upper(), int(leg[k])])
	legacy_lbl.text = "  |  ".join(legacy_parts)

	var output = res.get("output", null)
	if typeof(output) == TYPE_DICTIONARY:
		for beat in output.get("beats", []):
			var b_text := str(beat.get("text", ""))
			var b_clock := str(beat.get("clock", c))
			_append_log("[%dm] %s" % [int(b_clock), b_text])

	var ended = board.get("ended", null)
	if typeof(ended) == TYPE_DICTIONARY:
		_append_log("[color=#ffd43b]ENDING: %s (%s)[/color]" % [ended.get("title", ""), ended.get("tier", "")])

	# Update entity catalog
	entity_catalog.clear()
	for z in board.get("zones", []):
		for card in z.get("cards", []):
			for raw_id in card.get("entity_ids", []):
				entity_catalog[int(raw_id)] = card

	# The world moved; every cached staging view is stale.
	staging_cache.clear()

	# Refresh the situation window first (its staging refresh may prune staged
	# pairs back to the table), then the table.
	if not open_verb.is_empty():
		open_window(open_verb, current_page)
	_rebuild_table()

func _rebuild_table() -> void:
	var board: Dictionary = current_state.get("board", {})
	for child in verbs_flow.get_children():
		child.queue_free()

	# Map situations by verb
	var sits_by_verb := {}
	for s in board.get("situations", []):
		var vid := str(s.get("verb", ""))
		if not sits_by_verb.has(vid):
			sits_by_verb[vid] = []
		sits_by_verb[vid].append(s)

	# 1. Populate Verb Tokens
	var verbs: Array = current_state.get("verbs", [])
	# Autonomous first, then committable
	var sorted_verbs := []
	for v in verbs:
		if str(v.get("mode", "")) == "autonomous":
			sorted_verbs.append(v)
	for v in verbs:
		if str(v.get("mode", "")) != "autonomous":
			sorted_verbs.append(v)

	for v in sorted_verbs:
		var vid := str(v.get("id", ""))
		var sits: Array = sits_by_verb.get(vid, [])
		if str(v.get("mode", "")) == "uncommittable" and sits.is_empty():
			continue
		var token := VerbToken.new().setup(self, v, sits)
		verbs_flow.add_child(token)

	# 2. Populate Zone Trays and Cards
	for child in zones_section.get_children():
		child.queue_free()

	var zones: Array = board.get("zones", [])
	# Non-empty zones first
	var sorted_zones := []
	for z in zones:
		if not (z.get("cards", []) as Array).is_empty():
			sorted_zones.append(z)
	for z in zones:
		if (z.get("cards", []) as Array).is_empty():
			sorted_zones.append(z)

	var staged_set := _staged_set()

	for z in sorted_zones:
		var z_box := VBoxContainer.new()
		z_box.add_theme_constant_override("separation", 6)
		zones_section.add_child(z_box)

		var z_name := str(z.get("name", "")).to_upper()
		var cards: Array = z.get("cards", [])

		var z_hdr := UiKit.label("ZONE: %s" % z_name, 11, UiKit.TEXT_FAINT)
		z_box.add_child(z_hdr)

		var card_flow := ZoneCardFlow.new()
		z_box.add_child(card_flow)

		if cards.is_empty():
			var empty_msg := UiKit.label("No cards here.", 11, Color(UiKit.TEXT_FAINT, 0.4))
			card_flow.add_child(empty_msg)
		else:
			for card in cards:
				var raw_ids: Array = card.get("entity_ids", [card.get("entity_id", 0)])
				var locked_raw: Array = card.get("locked_ids", [])
				var locked_set := {}
				for lid in locked_raw:
					locked_set[int(lid)] = true

				var avail_ids := []
				for rid in raw_ids:
					var eid := int(rid)
					if not locked_set.has(eid) and not staged_set.has(eid):
						avail_ids.append(eid)

				var cv := CardView.new().setup(self, card, avail_ids, clock())
				card_flow.add_child(cv)

func _staged_set() -> Dictionary:
	var s := {}
	for pair in staged:
		s[int(pair[1])] = true
	return s

func _redraw_drop_targets() -> void:
	for node in get_tree().get_nodes_in_group("drop_target"):
		node.queue_redraw()

# --- Public Interface called by components ---

func clock() -> int:
	return int(current_state.get("board", {}).get("clock", 0))

func card_of(eid: int) -> Dictionary:
	return entity_catalog.get(eid, {})

func on_card_clicked(eid: int) -> void:
	var card_info := card_of(eid)
	var lore_res := bridge.request({"cmd": "inspect", "query": "#%d" % eid})
	var lore: Dictionary = lore_res.get("card", {}) if lore_res.get("ok", false) else {}
	inspector_card.inspect_card(card_info, lore, clock())

func on_drag_begin(eid: int) -> void:
	dragging_eid = eid
	_dragged_zone_card = _find_zone_card_view(eid)
	if _dragged_zone_card != null and _dragged_zone_card.get_parent() is ZoneCardFlow:
		_dragged_zone_card.get_parent().animate_card_removed(_dragged_zone_card)
	_redraw_drop_targets()

func _find_zone_card_view(eid: int) -> CardView:
	if zones_section == null:
		return null
	for z_box in zones_section.get_children():
		for flow in z_box.get_children():
			if flow is ZoneCardFlow:
				for cv in flow.get_children():
					if cv is CardView and cv.ids.has(eid):
						return cv
	return null

func open_window(verb_id: String, page_idx: int = 0) -> void:
	if verb_id != open_verb:
		staged = []
	open_verb = verb_id
	current_page = page_idx
	var v_dict: Dictionary = _find_verb(verb_id)
	if v_dict.is_empty():
		open_verb = ""
		situation_window.visible = false
		return

	var board: Dictionary = current_state.get("board", {})
	var sits := []
	for s in board.get("situations", []):
		if str(s.get("verb", "")) == verb_id:
			sits.append(s)

	var has_room := verb_has_room(v_dict)
	var total_pages := sits.size() + (1 if has_room else 0)
	if total_pages == 0:
		total_pages = 1
	current_page = clampi(current_page, 0, total_pages - 1)

	situation_window.build(self, v_dict, sits, current_page, total_pages)
	situation_window.visible = true
	_redraw_drop_targets()

func close_window() -> void:
	open_verb = ""
	staged = []
	situation_window.visible = false
	_rebuild_table()
	_redraw_drop_targets()

func set_page(idx: int) -> void:
	current_page = idx
	open_window(open_verb, current_page)

func verb_has_room(verb_dict: Dictionary) -> bool:
	if str(verb_dict.get("mode", "")) != "committable":
		return false
	var cap_str := str(verb_dict.get("instances", "1"))
	if cap_str == "any":
		return true
	var cap := int(cap_str)
	var current_count := 0
	for s in current_state.get("board", {}).get("situations", []):
		if str(s.get("verb", "")) == str(verb_dict.get("id", "")):
			current_count += 1
	return current_count < cap

func verb_accepts(verb_id: String, eid: int) -> bool:
	var v_dict := _find_verb(verb_id)
	if v_dict.is_empty():
		return false

	# If there's an active situation with an open slot accepting eid
	var board: Dictionary = current_state.get("board", {})
	var open_cands: Dictionary = current_state.get("open_slot_candidates", {})
	for s in board.get("situations", []):
		if str(s.get("verb", "")) == verb_id:
			var sid := int(s.get("id", -1))
			for os in s.get("open_slots", []):
				var key := "%d:%s" % [sid, str(os.get("name", ""))]
				var arr: Array = open_cands.get(key, [])
				if arr.has(eid) or arr.has(float(eid)):
					return true

	# Otherwise the card must fit some slot of the verb's latest staging view.
	# With no informative response yet (nothing matched, so no candidates are
	# known), accept and let the drop resolve.
	if verb_has_room(v_dict):
		var st: Dictionary = staging_cache.get(verb_id, {})
		if st.is_empty():
			st = staging_refresh(verb_id)
		if st.get("matched") == null and (st.get("slots", []) as Array).is_empty():
			return true
		for sl in st.get("slots", []):
			for c in sl.get("candidates", []):
				if int(c[0]) == eid:
					return true

	return false

func drop_on_verb(verb_id: String, eid: int) -> void:
	# If a running situation of this verb has an open slot accepting eid:
	var board: Dictionary = current_state.get("board", {})
	for s in board.get("situations", []):
		if str(s.get("verb", "")) == verb_id:
			var sid := int(s.get("id", -1))
			for os in s.get("open_slots", []):
				var sname := str(os.get("name", ""))
				if open_slot_accepts(sid, sname, eid):
					bind(sid, sname, eid)
					return

	# Otherwise stage the card for a new action on its new-action page; the
	# empty slot name asks the bridge to place the card where it fits.
	var v_dict := _find_verb(verb_id)
	if v_dict.is_empty() or not verb_has_room(v_dict):
		return
	if open_verb != verb_id:
		staged = []
	open_window(verb_id, 9999)
	stage("", eid)

func open_slot_accepts(sit_id: int, slot_name: String, eid: int) -> bool:
	var open_cands: Dictionary = current_state.get("open_slot_candidates", {})
	var key := "%d:%s" % [sit_id, slot_name]
	var arr: Array = open_cands.get(key, [])
	return arr.has(eid) or arr.has(float(eid))

func slot_accepts(slot_name: String, eid: int) -> bool:
	var st: Dictionary = staging_cache.get(open_verb, {})
	for sl in st.get("slots", []):
		if str(sl.get("name", "")) != slot_name:
			continue
		for c in sl.get("candidates", []):
			if int(c[0]) == eid:
				return true
	return false

func staging_refresh(verb_id: String) -> Dictionary:
	var bindings: Array = staged_bindings() if verb_id == open_verb else []
	var res := bridge.staging(verb_id, bindings)
	if not res.get("ok", false):
		return {}
	var st: Dictionary = res.get("staging", {})
	staging_cache[verb_id] = st
	if verb_id == open_verb:
		_prune_staged(st)
	return st

# Align staged pairs with a fresh staging view: a pair whose slot no longer
# renders moves to the first slot that still takes its card, and a pair
# nothing takes is pruned back to the table.
func _prune_staged(st: Dictionary) -> void:
	if staged.is_empty():
		return
	var kept := []
	for pair in staged:
		var sname := str(pair[0])
		var eid := int(pair[1])
		var placed := false
		for sl in st.get("slots", []):
			if placed or str(sl.get("name", "")) != sname:
				continue
			kept.append(pair)
			placed = true
		if placed:
			continue
		for sl in st.get("slots", []):
			var taken := false
			for c in sl.get("candidates", []):
				if int(c[0]) == eid:
					kept.append([str(sl.get("name", "")), eid])
					taken = true
					break
			if taken:
				placed = true
				break
		if not placed:
			_append_log("[color=#ff6b6b]Refused: %s does not take that card right now.[/color]" % open_verb)
	staged = kept

func stage(slot_name: String, eid: int) -> void:
	var kept := []
	for pair in staged:
		if int(pair[1]) != eid:
			kept.append(pair)
	kept.append([slot_name, eid])
	staged = kept
	_refresh_open_verb()

func unstage(eid: int) -> void:
	staged = staged.filter(func(pair: Array) -> bool: return int(pair[1]) != eid)
	_refresh_open_verb()

func _refresh_open_verb() -> void:
	if not open_verb.is_empty():
		open_window(open_verb, current_page)
	_rebuild_table()
	_redraw_drop_targets()

func bind(sit_id: int, slot_name: String, eid: int) -> void:
	_send({"cmd": "bind", "sit": sit_id, "slot": slot_name, "entity": eid})

func start() -> void:
	var vid := open_verb
	var bindings := staged.duplicate(true)
	staged = []
	var res := _send({"cmd": "commit", "verb": vid, "bindings": bindings})
	if not res.get("ok", false):
		staged = bindings
		open_window(vid, current_page)
		_rebuild_table()

func staged_bindings() -> Array:
	var out := []
	for pair in staged:
		out.append([str(pair[0]), int(pair[1])])
	return out

func preview_staged() -> Dictionary:
	return bridge.request({"cmd": "preview", "verb": open_verb, "bindings": staged_bindings()})

func staged_aspects_text() -> String:
	var totals := {}
	for pair in staged:
		var c := card_of(int(pair[1]))
		var aspects: Dictionary = c.get("aspects", {})
		for k in aspects.keys():
			totals[k] = totals.get(k, 0) + int(aspects[k])
	if totals.is_empty():
		return ""
	var parts := PackedStringArray()
	for k in totals.keys():
		parts.append("%s %d" % [k, totals[k]])
	return "ASPECTS: " + "  ·  ".join(parts)

func slot_capacity(sl: Dictionary) -> int:
	var bounds_str := str(sl.get("bounds", "1..1"))
	var parts := bounds_str.split("..")
	if parts.size() > 1 and not parts[1].is_empty():
		return int(parts[1])
	return 99

func _find_verb(vid: String) -> Dictionary:
	for v in current_state.get("verbs", []):
		if str(v.get("id", "")) == vid:
			return v
	return {}

func _append_log(msg: String) -> void:
	print("LOG: ", msg)
