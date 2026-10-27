class_name SlotSocket
extends Control
## A card-sized socket in a situation window. Holds staged (or bound) cards, accepts drops.

const OVERLAP := 18.0

var ui: Node
var capacity: int = 1
var accepts: Callable   # func(eid: int) -> bool
var on_drop: Callable   # func(eid: int) -> void

func setup(p_ui: Node, p_capacity: int, p_cards: Array, p_accepts: Callable, p_on_drop: Callable) -> SlotSocket:
	ui = p_ui
	capacity = p_capacity
	accepts = p_accepts
	on_drop = p_on_drop
	var n := maxi(1, p_cards.size())
	custom_minimum_size = Vector2(CardView.SIZE.x + OVERLAP * (n - 1), CardView.SIZE.y)
	mouse_filter = Control.MOUSE_FILTER_STOP
	for i in p_cards.size():
		var cv: CardView = p_cards[i]
		cv.position = Vector2(OVERLAP * i, 0)
		add_child(cv)
	add_to_group("drop_target")
	return self

func has_room() -> bool:
	return get_child_count() < capacity

func _draw() -> void:
	var r := Rect2(Vector2.ZERO, CardView.SIZE)
	if get_child_count() == 0:
		draw_style_box(UiKit.box(Color(0, 0, 0, 0.45), Color(UiKit.RING, 0.35), 1, 4), r)
		draw_style_box(UiKit.box(Color(0, 0, 0, 0), Color(UiKit.RING, 0.12), 1, 3), r.grow(-6))
	var deid: int = int(ui.get("dragging_eid")) if ui != null and ui.get("dragging_eid") != null else -1
	if deid >= 0 and has_room() and accepts.call(deid):
		draw_style_box(UiKit.box(Color(0, 0, 0, 0), UiKit.HILITE, 2, 6), Rect2(Vector2.ZERO, size).grow(4))

func _can_drop_data(_at: Vector2, data: Variant) -> bool:
	if typeof(data) != TYPE_DICTIONARY or data.get("type") != "card":
		return false
	return has_room() and bool(accepts.call(int(data.get("entity_id", -1))))

func _drop_data(_at: Vector2, data: Variant) -> void:
	on_drop.call(int(data.get("entity_id", -1)))
