# Chorus² — Delivery Plan

Scope, sequence, and acceptance criteria only. Architecture lives in spec.md; the content language in language.md; the implementation contract and test obligations in kernel-api.md. Sequencing principle: original sample content leads; the Cultist Simulator content converter follows as the expressiveness stress test and acceptance baseline.

## M0 — Language and Content Foundation

**Work.** Stand up the workspace with `story-core`, `story-content`, `story-lang` (crate map: spec.md §3). Implement the lexer, parser, IR, and the two-pass compiler per language.md — declarations, expressions, targets, effects, trigger books, `effectset` expansion, include splicing, meta pass-through. Implement the diagnostics system: every compile error in language.md §9 with `file:line:col`, stable number, and declaration-site note.

**Acceptance.**
- `emberwatch.sw` compiles with zero errors and zero unexpected warnings.
- Every diagnostic in language.md §9 fires on a fixture that triggers it, with location, number, and note.
- The compiled `Content` serializes and round-trips; fingerprinting is stable (kernel-api.md §2).

## M1 — Kernel

**Work.** Implement `story-engine` per kernel-api.md §1–§8: state and value types, the selection law and candidacy, materialization and slots, the settlement loop with phases and stepped now, the effect registry and all built-ins, trigger books including card-carried books, spawn handles, snapshots and boot.

**Acceptance.**
- `emberwatch.sw` is playable end to end through `survived` and through both loss endings.
- The snapshot/undo loop shows no state drift across a scripted session.
- Determinism holds: same seed + same operation sequence ⇒ identical state (kernel-api.md §9 T6).
- Completion order is observable as specified: recipe book first, then card books in bind order (kernel-api.md §9 T12).

## M2 — Mechanism Matrix

**Work.** Write the behavioral suite owning full mechanism coverage: every entry of kernel-api.md §10 (mechanisms the sample does not exercise, instant-recipe behaviors, new-mechanism entries) plus every testable invariant T1–T14 (kernel-api.md §9).

**Acceptance.**
- Every matrix entry has at least one positive path and one refusal path, green.
- T1–T14 each carry an executable assertion, green.
- The suite runs as a single command and is part of the default check.

## M3 — Converter

**Work.** Build `cs2sw`: Cultist Simulator source JSON → `.sw`, exploiting the grammar's mechanical regularity (language.md design goals) — the converter maps, it never interprets.

**Acceptance.**
- Converted Cultist Simulator content passes compilation cleanly.
- A playability smoke runs the converted content through a scripted sequence of commits and advances without engine errors.
- The conversion stands as the expressiveness stress test: any construct the converter needs and the language lacks surfaces as a named gap to route through spec.md §7 (extension seams) or §6 (non-goals).
