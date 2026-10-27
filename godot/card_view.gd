class_name CardView
extends Control
## A card drawn as an art field over a paper label, with stack badge and decay timer.

const SIZE := Vector2(86, 120)

var ui: Node
var card: Dictionary = {}
var ids: Array = []          # entity ids still available on the table (top first)
var clock: int = 0
var from_slot: String = ""   # set when the card sits in a staged slot
var selected: bool = false
var draggable: bool = true
var _press_pos: Vector2 = Vector2.ZERO
var _is_pressed: bool = false
var _drag_started: bool = false

func _notification(what: int) -> void:
	if what == NOTIFICATION_DRAG_END:
		_is_pressed = false
		_drag_started = false

func setup(p_ui: Node, p_card: Dictionary, p_ids: Array, p_clock: int) -> CardView:
	ui = p_ui
	card = p_card
	ids = p_ids
	clock = p_clock
	custom_minimum_size = SIZE
	size = SIZE
	mouse_filter = Control.MOUSE_FILTER_STOP
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	if ids.is_empty():
		modulate = Color(1, 1, 1, 0.28)
	return self

func top_id() -> int:
	return int(ids[0]) if not ids.is_empty() else int(card.get("entity_id", -1))

func _draw() -> void:
	var r := Rect2(Vector2.ZERO, size)
	draw_rect(Rect2(Vector2(2, 3), size), Color(0, 0, 0, 0.45))
	var edge := UiKit.HILITE if selected else Color(0, 0, 0, 0.7)
	draw_style_box(UiKit.box(UiKit.PAPER, edge, 2 if selected else 1, 3), r)

	var art := Rect2(4, 4, size.x - 8, size.y * 0.58)
	draw_rect(art, UiKit.card_color(card))
	var kind := str(card.get("kind")) if card.get("kind") != null else ""
	if not kind.is_empty():
		UiKit.text_block(self, art.grow(-6), kind.to_upper(), 9, Color(1, 1, 1, 0.6), 2)

	var label_rect := Rect2(5, art.end.y + 2, size.x - 10, size.y - art.end.y - 6)
	var title := str(card.get("title", card.get("prototype", "?"))).capitalize()
	UiKit.text_block(self, label_rect, title, 11, UiKit.PAPER_INK, 3)

	if card.get("expires_at") != null:
		var rem := maxi(0, int(card.get("expires_at")) - clock)
		UiKit.pill(self, Vector2(size.x * 0.5, 16), "%dm" % rem, 10)
	if ids.size() > 1:
		UiKit.badge(self, Vector2(size.x - 4, size.y - 4), ids.size())

func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_LEFT:
			if event.pressed:
				_is_pressed = true
				_press_pos = event.global_position
				_drag_started = false
			else:
				if _is_pressed and not _drag_started:
					_is_pressed = false
					if event.global_position.distance_to(_press_pos) < 6.0:
						ui.on_card_clicked(top_id())
				_is_pressed = false
				_drag_started = false
		elif event.button_index == MOUSE_BUTTON_RIGHT and event.pressed:
			if not from_slot.is_empty():
				ui.unstage(top_id())
			elif not ui.open_verb.is_empty():
				ui.drop_on_verb(ui.open_verb, top_id())

	elif event is InputEventMouseMotion and _is_pressed and not _drag_started:
		if event.global_position.distance_to(_press_pos) > 4.0:
			if not ids.is_empty() and draggable:
				_drag_started = true
				_start_drag()

func _start_drag() -> void:
	var eid := int(ids[0])
	var ghost := CardView.new().setup(ui, card, [eid], clock)
	ghost.mouse_filter = Control.MOUSE_FILTER_IGNORE
	ghost.position = -SIZE * 0.5
	var holder := Control.new()
	holder.mouse_filter = Control.MOUSE_FILTER_IGNORE
	holder.add_child(ghost)
	if ui != null and ui.has_method("on_drag_begin"):
		ui.on_drag_begin(eid)
	force_drag({"type": "card", "entity_id": eid, "from_slot": from_slot}, holder)

func _get_drag_data(_at: Vector2) -> Variant:
	if ids.is_empty() or not draggable:
		return null
	_is_pressed = false
	_drag_started = true
	var eid := int(ids[0])
	var ghost := CardView.new().setup(ui, card, [eid], clock)
	ghost.mouse_filter = Control.MOUSE_FILTER_IGNORE
	ghost.position = -SIZE * 0.5
	var holder := Control.new()
	holder.mouse_filter = Control.MOUSE_FILTER_IGNORE
	holder.add_child(ghost)
	set_drag_preview(holder)
	if ui != null and ui.has_method("on_drag_begin"):
		ui.on_drag_begin(eid)
	return {"type": "card", "entity_id": eid, "from_slot": from_slot}
