class_name UiKit
extends RefCounted
## Shared palette, style boxes, and text drawing for the tabletop UI.

const FELT_OUTER := Color(0.06, 0.11, 0.12)
const FELT := Color(0.11, 0.21, 0.23)
const FELT_EDGE := Color(0.17, 0.30, 0.32)
const PAPER := Color(0.93, 0.90, 0.82)
const PAPER_INK := Color(0.17, 0.14, 0.12)
const PAPER_FAINT := Color(0.42, 0.37, 0.31)
const WINDOW_BG := Color(0.05, 0.10, 0.11, 0.96)
const WINDOW_EDGE := Color(0.38, 0.58, 0.61)
const WINDOW_TITLE := Color(0.78, 0.92, 0.93)
const TEXT := Color(0.86, 0.90, 0.89)
const TEXT_FAINT := Color(0.55, 0.64, 0.64)
const RING := Color(0.66, 0.88, 0.91)
const HILITE := Color(0.98, 0.86, 0.52)
const TOKEN_FRAME := Color(0.13, 0.19, 0.20)
const TOKEN_FACE := Color(0.24, 0.42, 0.45)
const TOKEN_FACE_AUTO := Color(0.46, 0.27, 0.22)

static func box(bg: Color, border: Color = Color(0, 0, 0, 0), bw: int = 0, radius: int = 4, pad: int = 0) -> StyleBoxFlat:
	var sb := StyleBoxFlat.new()
	sb.bg_color = bg
	sb.border_color = border
	sb.set_border_width_all(bw)
	sb.set_corner_radius_all(radius)
	sb.set_content_margin_all(pad)
	sb.anti_aliasing = true
	return sb

## Card art color: a stable hue per kind (or prototype when the card has no kind).
static func card_color(card: Dictionary) -> Color:
	var key := str(card.get("kind")) if card.get("kind") != null else str(card.get("prototype", ""))
	var hue := float(absi(key.hash()) % 360) / 360.0
	return Color.from_hsv(hue, 0.42, 0.72)

static func font() -> Font:
	return ThemeDB.fallback_font

## Draws centered, wrapped text inside `rect`.
static func text_block(ci: CanvasItem, rect: Rect2, text: String, size: int, color: Color, max_lines: int = 3) -> void:
	var f := font()
	var dims := f.get_multiline_string_size(text, HORIZONTAL_ALIGNMENT_CENTER, rect.size.x, size, max_lines)
	var y := rect.position.y + maxf(0.0, (rect.size.y - dims.y) * 0.5) + f.get_ascent(size)
	ci.draw_multiline_string(f, Vector2(rect.position.x, y), text, HORIZONTAL_ALIGNMENT_CENTER, rect.size.x, size, max_lines, color)

## Small dark pill with a centered label (timers, counters).
static func pill(ci: CanvasItem, center: Vector2, text: String, size: int = 11) -> void:
	var f := font()
	var w := f.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, size).x + 12.0
	var r := Rect2(center - Vector2(w * 0.5, 9), Vector2(w, 18))
	ci.draw_style_box(box(Color(0.02, 0.04, 0.05, 0.92), Color(1, 1, 1, 0.25), 1, 9), r)
	ci.draw_string(f, Vector2(r.position.x, r.position.y + 13), text, HORIZONTAL_ALIGNMENT_CENTER, w, size, Color(0.95, 0.95, 0.93))

## Round count badge (stack size, instance count).
static func badge(ci: CanvasItem, center: Vector2, n: int) -> void:
	ci.draw_circle(center, 10.0, Color(0.06, 0.08, 0.09))
	ci.draw_arc(center, 10.0, 0.0, TAU, 24, Color(0.85, 0.85, 0.80), 1.5, true)
	ci.draw_string(font(), center + Vector2(-10, 4), str(n), HORIZONTAL_ALIGNMENT_CENTER, 20, 11, Color(0.95, 0.95, 0.92))

static func make_theme() -> Theme:
	var t := Theme.new()
	t.set_color("font_color", "Label", TEXT)
	t.set_stylebox("normal", "Button", box(PAPER, Color(0, 0, 0, 0.4), 1, 12, 6))
	t.set_stylebox("hover", "Button", box(Color(0.99, 0.96, 0.88), HILITE, 1, 12, 6))
	t.set_stylebox("pressed", "Button", box(Color(0.82, 0.78, 0.68), Color(0, 0, 0, 0.5), 1, 12, 6))
	t.set_stylebox("disabled", "Button", box(Color(0.40, 0.42, 0.40, 0.6), Color(0, 0, 0, 0.3), 1, 12, 6))
	t.set_stylebox("focus", "Button", StyleBoxEmpty.new())
	t.set_color("font_color", "Button", PAPER_INK)
	t.set_color("font_hover_color", "Button", PAPER_INK)
	t.set_color("font_pressed_color", "Button", PAPER_INK)
	t.set_color("font_disabled_color", "Button", Color(0.18, 0.20, 0.20))
	t.set_font_size("font_size", "Button", 13)
	t.set_color("default_color", "RichTextLabel", TEXT)
	return t

## Flat text-only button (window close, pager arrows).
static func flat_button(text: String, color: Color = WINDOW_TITLE) -> Button:
	var b := Button.new()
	b.text = text
	b.flat = true
	for s in ["normal", "hover", "pressed", "disabled", "focus"]:
		b.add_theme_stylebox_override(s, StyleBoxEmpty.new())
	b.add_theme_color_override("font_color", color)
	b.add_theme_color_override("font_hover_color", HILITE)
	b.add_theme_color_override("font_pressed_color", HILITE)
	b.add_theme_font_size_override("font_size", 16)
	return b

static func label(text: String, size: int = 13, color: Color = TEXT) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	return l
