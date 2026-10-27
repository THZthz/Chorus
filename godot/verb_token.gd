class_name VerbToken
extends Control
## A verb drawn as a framed square tile; running situations add a progress ring and timer.

const SIZE := Vector2(100, 100)

var ui: Node
var verb: Dictionary = {}
var sits: Array = []   # running situations of this verb

func setup(p_ui: Node, p_verb: Dictionary, p_sits: Array) -> VerbToken:
	ui = p_ui
	verb = p_verb
	sits = p_sits
	custom_minimum_size = SIZE
	size = SIZE
	mouse_filter = Control.MOUSE_FILTER_STOP
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	add_to_group("drop_target")
	return self

func verb_id() -> String:
	return str(verb.get("id", ""))

func _draw() -> void:
	var frame := Rect2(Vector2.ZERO, size)
	var auto := str(verb.get("mode", "")) == "autonomous"
	var idle := sits.is_empty()

	if not idle:
		var s: Dictionary = sits[0]
		var dur := maxf(1.0, float(s.get("total_duration", 1)))
		var progress := 1.0 - float(s.get("remaining_time", 0)) / dur
		var c := size * 0.5
		draw_arc(c, 66.0, 0.0, TAU, 72, Color(UiKit.RING, 0.18), 3.0, true)
		if progress > 0.0:
			draw_arc(c, 66.0, -PI * 0.5, -PI * 0.5 + TAU * progress, 72, UiKit.RING, 3.0, true)

	draw_rect(Rect2(Vector2(3, 4), size), Color(0, 0, 0, 0.45))
	draw_style_box(UiKit.box(UiKit.TOKEN_FRAME, Color(0, 0, 0, 0.8), 1, 6), frame)
	var face_col := UiKit.TOKEN_FACE_AUTO if auto else UiKit.TOKEN_FACE
	if idle and auto:
		face_col = face_col.darkened(0.35)
	var face := frame.grow(-10)
	draw_style_box(UiKit.box(face_col, face_col.lightened(0.3), 1, 3), face)
	UiKit.text_block(self, face.grow(-5), str(verb.get("title", verb_id())), 12, Color(0.97, 0.96, 0.92), 4)

	var hot: bool = ui.dragging_eid >= 0 and ui.verb_accepts(verb_id(), ui.dragging_eid)
	if hot:
		draw_style_box(UiKit.box(Color(0, 0, 0, 0), UiKit.HILITE, 2, 8), frame.grow(5))
	elif ui.open_verb == verb_id():
		draw_style_box(UiKit.box(Color(0, 0, 0, 0), UiKit.RING, 1, 8), frame.grow(4))

	if not idle:
		UiKit.pill(self, Vector2(size.x * 0.5, -20), "%dm" % int(sits[0].get("remaining_time", 0)))
	if sits.size() > 1:
		UiKit.badge(self, Vector2(size.x - 2, size.y - 2), sits.size())

func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		ui.open_window(verb_id())

func _can_drop_data(_at: Vector2, data: Variant) -> bool:
	return typeof(data) == TYPE_DICTIONARY and data.get("type") == "card" \
		and ui.verb_accepts(verb_id(), int(data.get("entity_id", -1)))

func _drop_data(_at: Vector2, data: Variant) -> void:
	ui.drop_on_verb(verb_id(), int(data.get("entity_id", -1)))
