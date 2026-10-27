class_name InspectorCard
extends PanelContainer
## Cultist Simulator-style parchment index card pinned to the top-right corner.

var title_lbl: Label
var kind_lbl: Label
var desc_lbl: RichTextLabel
var aspects_flow: HFlowContainer
var meta_lbl: Label

func _init() -> void:
	custom_minimum_size = Vector2(340, 240)
	size_flags_horizontal = Control.SIZE_SHRINK_END
	size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	mouse_filter = Control.MOUSE_FILTER_STOP

	# Parchment note style with dark drop shadow
	var sb := StyleBoxFlat.new()
	sb.bg_color = UiKit.PAPER
	sb.border_color = Color(0.15, 0.12, 0.10, 0.8)
	sb.set_border_width_all(1)
	sb.set_corner_radius_all(3)
	sb.set_content_margin_all(14)
	sb.shadow_color = Color(0, 0, 0, 0.6)
	sb.shadow_size = 8
	sb.shadow_offset = Vector2(2, 4)
	add_theme_stylebox_override("panel", sb)

	var root := VBoxContainer.new()
	root.add_theme_constant_override("separation", 8)
	add_child(root)

	# Header row
	var h_row := HBoxContainer.new()
	root.add_child(h_row)

	var tab := Label.new()
	tab.text = "[CARD]"
	tab.add_theme_font_size_override("font_size", 10)
	tab.add_theme_color_override("font_color", UiKit.PAPER_FAINT)
	h_row.add_child(tab)

	title_lbl = Label.new()
	title_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	title_lbl.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	title_lbl.add_theme_font_size_override("font_size", 15)
	title_lbl.add_theme_color_override("font_color", UiKit.PAPER_INK)
	h_row.add_child(title_lbl)

	var close_btn := Button.new()
	close_btn.text = " [X] "
	close_btn.flat = true
	close_btn.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	close_btn.add_theme_font_size_override("font_size", 12)
	close_btn.add_theme_color_override("font_color", UiKit.PAPER_FAINT)
	close_btn.add_theme_color_override("font_hover_color", Color(0.6, 0.15, 0.15))
	close_btn.pressed.connect(func() -> void: visible = false)
	h_row.add_child(close_btn)

	kind_lbl = Label.new()
	kind_lbl.add_theme_font_size_override("font_size", 10)
	kind_lbl.add_theme_color_override("font_color", UiKit.PAPER_FAINT)
	root.add_child(kind_lbl)

	var rule := ColorRect.new()
	rule.custom_minimum_size.y = 1
	rule.color = Color(UiKit.PAPER_INK, 0.25)
	root.add_child(rule)

	# Description
	desc_lbl = RichTextLabel.new()
	desc_lbl.bbcode_enabled = true
	desc_lbl.fit_content = true
	desc_lbl.custom_minimum_size.y = 60
	desc_lbl.add_theme_font_size_override("normal_font_size", 12)
	desc_lbl.add_theme_font_size_override("italic_font_size", 12)
	desc_lbl.add_theme_color_override("default_color", UiKit.PAPER_INK)
	root.add_child(desc_lbl)

	# Aspect chips
	aspects_flow = HFlowContainer.new()
	aspects_flow.add_theme_constant_override("h_separation", 6)
	aspects_flow.add_theme_constant_override("v_separation", 4)
	root.add_child(aspects_flow)

	# Relations / Decay footer
	meta_lbl = Label.new()
	meta_lbl.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	meta_lbl.add_theme_font_size_override("font_size", 11)
	meta_lbl.add_theme_color_override("font_color", UiKit.PAPER_FAINT)
	root.add_child(meta_lbl)

func inspect_card(card: Dictionary, lore: Dictionary, clock: int) -> void:
	visible = true
	var title := str(lore.get("title", card.get("title", card.get("prototype", "Unknown")))).capitalize()
	var eid_val: Variant = lore.get("entity_id", card.get("entity_id"))
	if eid_val != null:
		title_lbl.text = "%s (#%d)" % [title, int(eid_val)]
	else:
		title_lbl.text = title

	var kind_str := str(lore.get("kind", card.get("kind", "")))
	kind_lbl.text = kind_str.to_upper() if not kind_str.is_empty() else "CARD"

	var desc := str(lore.get("description", ""))
	if desc.is_empty():
		desc = "No recorded lore for this matter."
	desc_lbl.text = "[i]%s[/i]" % desc

	# Rebuild aspect chips
	for c in aspects_flow.get_children():
		c.queue_free()

	var aspects: Dictionary = lore.get("aspects", card.get("aspects", {}))
	for k in aspects.keys():
		var chip := PanelContainer.new()
		var chip_sb := StyleBoxFlat.new()
		chip_sb.bg_color = Color(0.20, 0.17, 0.14)
		chip_sb.border_color = Color(0.45, 0.38, 0.30)
		chip_sb.set_border_width_all(1)
		chip_sb.set_corner_radius_all(3)
		chip_sb.set_content_margin_all(4)
		chip.add_theme_stylebox_override("panel", chip_sb)

		var chip_lbl := Label.new()
		chip_lbl.text = "%s %d" % [k, int(aspects[k])]
		chip_lbl.add_theme_font_size_override("font_size", 10)
		chip_lbl.add_theme_color_override("font_color", UiKit.PAPER)
		chip.add_child(chip_lbl)
		aspects_flow.add_child(chip)

	# Expiry / relations
	var meta_lines := PackedStringArray()
	if card.get("expires_at") != null:
		var rem := maxi(0, int(card.get("expires_at")) - clock)
		meta_lines.append("Decays in %dm (at %dm)." % [rem, int(card.get("expires_at"))])

	var rels: Array = lore.get("relations", [])
	for r in rels:
		meta_lines.append("Relation: %s -> %s" % [str(r[0]), str(r[1])])

	meta_lbl.text = "\n".join(meta_lines)
