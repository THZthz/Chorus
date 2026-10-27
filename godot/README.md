# Godot Tabletop Visualizer

Tabletop client for the Chorus² card simulation engine, inspired by *Cultist Simulator* and *Book of Hours*.

## Architecture

The visualizer connects directly to the compiled Rust engine via a subprocess stdio pipe running `story-cli bridge`. All game state, recipe candidate calculations, and commitments are performed by the Rust kernel; Godot acts as a thin presentation and interaction surface.

```
+-----------------------------------------------------------+
|                        Godot 4                            |
|  (TableCanvas, VerbToken, CardView, SituationWindow, ...) |
+-----------------------------+-----------------------------+
                              |
                     NDJSON over stdio
                              |
+-----------------------------v-----------------------------+
|                      story-cli bridge                     |
|           (Chorus kernel & Presenter cascade)             |
+-----------------------------------------------------------+
```

## Structure

- [`project.godot`](project.godot): Godot 4 project configuration (1440x900 viewport, 2D canvas items).
- [`main.gd`](main.gd): Root scene manager orchestrating tabletop layout, bridge requests, overlays, and keyboard shortcuts.
- [`bridge.gd`](bridge.gd): Manages the `story-cli bridge` child process and handles NDJSON request/response pipes.
- [`ui_kit.gd`](ui_kit.gd): Theme definitions, color palettes (felt, parchment, gold, card categories), styleboxes, and text helpers.
- [`card_view.gd`](card_view.gd): Physical card widget with colored art headers, title, kind label, decay timer pill, stack badge, and drag-and-drop previews.
- [`verb_token.gd`](verb_token.gd): Square verb tile with circular progress rings, countdown pills, and drop targeting.
- [`slot_socket.gd`](slot_socket.gd): Sockets for the situation window with dashed/inset borders, drag-and-drop reception, and card slotting.
- [`situation_window.gd`](situation_window.gd): Draggable pop-up situation window with narrative chamber, recipe title, action button, and slot sockets.
- [`inspector_card.gd`](inspector_card.gd): Pinned parchment index card displaying card lore, title, entity ID, kind, aspect chips, and decay timers.
- [`table_canvas.gd`](table_canvas.gd): Tabletop felt canvas and unstage drop target.

## Usage

```bash
godot --path godot
godot --path godot -- docs/emberwatch.sw
```
