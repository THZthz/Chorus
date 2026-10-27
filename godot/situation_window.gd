class_name SituationWindow
extends PanelContainer
## The verb window. Pages: one per running situation, then the new-action page when the verb has room.

var ui: Node
var _is_dragging: bool = false
var _drag_start_mouse: Vector2 = Vector2.ZERO
var _drag_start_pos: Vector2 = Vector2.ZERO

func _gui_input(event: InputEvent) -> void:
	_handle_drag(event)

func _handle_drag(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_LEFT:
		if event.pressed:
			_is_dragging = true
			_drag_start_mouse = event.global_position
			_drag_start_pos = position
			accept_event()
		else:
			_is_dragging = false
			accept_event()
	elif event is InputEventMouseMotion and _is_dragging:
		position = _drag_start_pos + (event.global_position - _drag_start_mouse)
		accept_event()

func _input(event: InputEvent) -> void:
	if event is InputEventMouseButton and not event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		_is_dragging = false

func _init() -> void:
	add_theme_stylebox_override("panel", UiKit.box(UiKit.WINDOW_BG, UiKit.WINDOW_EDGE, 1, 4, 14))
	custom_minimum_size = Vector2(680, 0)
	mouse_filter = Control.MOUSE_FILTER_STOP

func build(p_ui: Node, verb: Dictionary, sits: Array, page: int, pages: int) -> void:
	ui = p_ui
	for c in get_children():
		c.queue_free()

	var running := page < sits.size()
	var sit: Dictionary = sits[page] if running else {}
	var preview: Dictionary = {} if running else ui.preview_staged()
	var matched := bool(preview.get("matched", false))
	var desc := str(verb.get("description")) if verb.get("description") != null else ""

	var title := str(verb.get("title", verb.get("id", ""))).capitalize()
	var prose := desc
	var note := ""
	if running:
		title = str(sit.get("recipe_label", title)).capitalize()
		if sit.get("start_text") != null:
			prose = str(sit.get("start_text"))
		note = "Ends at %dm." % int(sit.get("ends_at", 0))
	elif not ui.verb_has_room(verb):
		note = "This verb starts on its own." if str(verb.get("mode", "")) == "autonomous" else "No room for another action here."
	elif matched:
		title = str(preview.get("label", title)).capitalize()
		if preview.get("prose") != null:
			prose = str(preview.get("prose"))
		note = "Takes %dm." % int(preview.get("duration", 0))
	elif str(preview.get("kind", "")) == "GuardRefused":
		note = str(preview.get("reason", ""))
	elif not ui.staged.is_empty():
		note = "Nothing will come of this."

	var root := VBoxContainer.new()
	root.add_theme_constant_override("separation", 10)
	add_child(root)

	# Header (drag handle)
	var header := HBoxContainer.new()
	header.mouse_filter = Control.MOUSE_FILTER_STOP
	header.gui_input.connect(_handle_drag)
	root.add_child(header)
	var title_lbl := UiKit.label(title, 19, UiKit.WINDOW_TITLE)
	title_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	title_lbl.mouse_filter = Control.MOUSE_FILTER_PASS
	header.add_child(title_lbl)

	var close := Button.new()
	close.text = " [X] "
	close.custom_minimum_size = Vector2(36, 28)
	close.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	close.add_theme_stylebox_override("normal", UiKit.box(Color(0, 0, 0, 0.35), UiKit.WINDOW_EDGE, 1, 3, 4))
	close.add_theme_stylebox_override("hover", UiKit.box(Color(0.35, 0.1, 0.1), UiKit.HILITE, 1, 3, 4))
	close.add_theme_stylebox_override("pressed", UiKit.box(Color(0.45, 0.1, 0.1), UiKit.HILITE, 1, 3, 4))
	close.add_theme_color_override("font_color", UiKit.WINDOW_TITLE)
	close.add_theme_color_override("font_hover_color", Color(1.0, 0.8, 0.8))
	close.add_theme_font_size_override("font_size", 12)
	close.pressed.connect(func() -> void: ui.close_window())
	header.add_child(close)

	var rule := ColorRect.new()
	rule.color = Color(UiKit.WINDOW_EDGE, 0.6)
	rule.custom_minimum_size.y = 1
	root.add_child(rule)

	# Body: prose on the left, slots on the right
	var body := HBoxContainer.new()
	body.add_theme_constant_override("separation", 18)
	root.add_child(body)

	var left := VBoxContainer.new()
	left.custom_minimum_size.x = 260
	body.add_child(left)
	var prose_box := PanelContainer.new()
	prose_box.add_theme_stylebox_override("panel", UiKit.box(Color(0, 0, 0, 0.35), Color(0, 0, 0, 0), 0, 4, 12))
	prose_box.custom_minimum_size.y = 180
	prose_box.size_flags_vertical = Control.SIZE_EXPAND_FILL
	left.add_child(prose_box)
	var rt := RichTextLabel.new()
	rt.bbcode_enabled = true
	rt.add_theme_font_size_override("normal_font_size", 14)
	var text := "[i]%s[/i]" % prose if not prose.is_empty() else ""
	if not note.is_empty():
		text += ("\n\n" if not text.is_empty() else "") + "[color=#%s]%s[/color]" % [UiKit.TEXT_FAINT.to_html(false), note]
	rt.text = text
	prose_box.add_child(rt)

	if pages > 1:
		var pager := HBoxContainer.new()
		pager.alignment = BoxContainer.ALIGNMENT_CENTER
		left.add_child(pager)
		var prev := UiKit.flat_button("<")
		prev.disabled = page == 0
		prev.pressed.connect(func() -> void: ui.set_page(page - 1))
		pager.add_child(prev)
		pager.add_child(UiKit.label("%d / %d" % [page + 1, pages], 12, UiKit.TEXT_FAINT))
		var next := UiKit.flat_button(">")
		next.disabled = page >= pages - 1
		next.pressed.connect(func() -> void: ui.set_page(page + 1))
		pager.add_child(next)

	var right := HBoxContainer.new()
	right.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right.alignment = BoxContainer.ALIGNMENT_CENTER
	right.add_theme_constant_override("separation", 14)
	body.add_child(right)

	if running:
		_build_running(right, sit)
	elif ui.verb_has_room(verb):
		_build_new(right, verb)

	# Footer: aspect strip, close button, and action button
	var footer := HBoxContainer.new()
	footer.add_theme_constant_override("separation", 10)
	root.add_child(footer)
	var aspects := UiKit.label("" if running else ui.staged_aspects_text(), 12, UiKit.HILITE)
	aspects.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	footer.add_child(aspects)

	var close_btn := Button.new()
	close_btn.text = "Close"
	close_btn.custom_minimum_size = Vector2(80, 32)
	close_btn.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	close_btn.add_theme_stylebox_override("normal", UiKit.box(Color(0.12, 0.13, 0.16), UiKit.WINDOW_EDGE, 1, 3, 4))
	close_btn.add_theme_stylebox_override("hover", UiKit.box(Color(0.2, 0.22, 0.28), UiKit.HILITE, 1, 3, 4))
	close_btn.add_theme_color_override("font_color", UiKit.TEXT)
	close_btn.pressed.connect(func() -> void: ui.close_window())
	footer.add_child(close_btn)

	var action := Button.new()
	action.custom_minimum_size = Vector2(100, 32)
	if running:
		action.text = "Running"
		action.disabled = true
	else:
		action.text = "Start"
		action.disabled = not matched
		action.pressed.connect(ui.start)
	footer.add_child(action)

func _build_new(parent: Control, verb: Dictionary) -> void:
	var slots: Array = ui.slot_union(verb)
	if slots.is_empty():
		parent.add_child(UiKit.label("No cards needed.", 12, UiKit.TEXT_FAINT))
		return
	for sl in slots:
		var s_name := str(sl.get("name", ""))
		var staged_cards := []
		for eid in ui.staged.get(s_name, []):
			var cv := CardView.new().setup(ui, ui.card_of(eid), [eid], ui.clock())
			cv.from_slot = s_name
			staged_cards.append(cv)
		var socket := SlotSocket.new().setup(
			ui, ui.slot_capacity(sl), staged_cards,
			func(eid: int) -> bool: return ui.slot_accepts(verb, s_name, eid),
			func(eid: int) -> void: ui.stage(s_name, eid)
		)
		parent.add_child(_slot_column(sl, socket))

func _build_running(parent: Control, sit: Dictionary) -> void:
	parent.add_child(TimerRing.new(sit))
	var sit_id := int(sit.get("id", 0))
	var by_slot := {}
	for sc in sit.get("slotted", []):
		var key := str(sc.get("slot", ""))
		if not by_slot.has(key):
			by_slot[key] = []
		by_slot[key].append(sc)

	var open_names := {}
	for os in sit.get("open_slots", []):
		var s_name := str(os.get("name", ""))
		open_names[s_name] = true
		var held := []
		for sc in by_slot.get(s_name, []):
			held.append(_bound_card(sc))
		var socket := SlotSocket.new().setup(
			ui, ui.slot_capacity(os), held,
			func(eid: int) -> bool: return ui.open_slot_accepts(sit_id, s_name, eid),
			func(eid: int) -> void: ui.bind(sit_id, s_name, eid)
		)
		parent.add_child(_slot_column(os, socket))

	for s_name in by_slot.keys():
		if open_names.has(s_name):
			continue
		var col := VBoxContainer.new()
		col.add_theme_constant_override("separation", 4)
		col.add_child(_caption(str(s_name).to_upper(), 10, UiKit.TEXT_FAINT))
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 6)
		for sc in by_slot[s_name]:
			row.add_child(_bound_card(sc))
		col.add_child(row)
		parent.add_child(col)

func _bound_card(sc: Dictionary) -> CardView:
	var card := {"title": sc.get("title", ""), "prototype": sc.get("prototype", ""), "entity_id": sc.get("entity_id", 0)}
	var known: Dictionary = ui.card_of(int(sc.get("entity_id", 0)))
	if not known.is_empty():
		card["kind"] = known.get("kind")
	var cv := CardView.new().setup(ui, card, [int(sc.get("entity_id", 0))], ui.clock())
	cv.draggable = false
	return cv

class SlotColumn extends VBoxContainer:
	var socket: SlotSocket
	func _can_drop_data(at: Vector2, data: Variant) -> bool:
		return socket != null and socket._can_drop_data(at, data)

	func _drop_data(at: Vector2, data: Variant) -> void:
		if socket != null:
			socket._drop_data(at, data)

func _slot_column(sl: Dictionary, socket: SlotSocket) -> VBoxContainer:
	var col := SlotColumn.new()
	col.socket = socket
	col.mouse_filter = Control.MOUSE_FILTER_STOP
	col.add_theme_constant_override("separation", 4)
	col.add_child(_caption(str(sl.get("label", sl.get("name", ""))).to_upper(), 10, UiKit.TEXT))
	var holder := CenterContainer.new()
	holder.mouse_filter = Control.MOUSE_FILTER_PASS
	holder.add_child(socket)
	col.add_child(holder)
	col.add_child(_caption(str(sl.get("criteria", "any")), 10, UiKit.TEXT_FAINT))
	var flags := str(sl.get("flags", ""))
	if not flags.is_empty():
		col.add_child(_caption(flags, 9, Color(UiKit.HILITE, 0.8)))
	return col

func _caption(text: String, size: int, color: Color) -> Label:
	var l := UiKit.label(text, size, color)
	l.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	l.custom_minimum_size.x = CardView.SIZE.x + 14
	l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	return l




class TimerRing extends Control:
	var sit: Dictionary

	func _init(p_sit: Dictionary) -> void:
		sit = p_sit
		custom_minimum_size = Vector2(130, 130)

	func _draw() -> void:
		var c := size * 0.5
		var dur := maxf(1.0, float(sit.get("total_duration", 1)))
		var rem := int(sit.get("remaining_time", 0))
		var progress := 1.0 - float(rem) / dur
		draw_arc(c, 54.0, 0.0, TAU, 72, Color(UiKit.RING, 0.18), 4.0, true)
		if progress > 0.0:
			draw_arc(c, 54.0, -PI * 0.5, -PI * 0.5 + TAU * progress, 72, UiKit.RING, 4.0, true)
		var f := UiKit.font()
		draw_string(f, c + Vector2(-60, 6), "%dm" % rem, HORIZONTAL_ALIGNMENT_CENTER, 120, 24, UiKit.WINDOW_TITLE)
		draw_string(f, c + Vector2(-60, 26), "remaining", HORIZONTAL_ALIGNMENT_CENTER, 120, 10, UiKit.TEXT_FAINT)
