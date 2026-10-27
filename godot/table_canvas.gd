class_name TableCanvas
extends Control
## The felt surface holding verb tokens and cards; takes cards back from slots.

var ui: Node

func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_PASS

func _can_drop_data(_at: Vector2, data: Variant) -> bool:
	return typeof(data) == TYPE_DICTIONARY and data.get("type") == "card" and not str(data.get("from_slot", "")).is_empty()

func _drop_data(_at: Vector2, data: Variant) -> void:
	ui.unstage(int(data.get("entity_id", -1)))
