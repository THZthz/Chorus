# Chorus

A text-only card-game prototype — Cultist Simulator / Book of Hours-shaped
games expressed entirely as sentences, deterministic end to end.

Build:

```sh
cargo build --release     # the bins land in target/release/
```

Play (line REPL):

```sh
cargo run --bin chorus -- scripts/the-nightjar-flyer.txt   # or any script file
```

A real save, replayed by the analyzer — inside the REPL `save <file>` writes
the action log, and `reach --from-state` reads it back as a midgame world:

```sh
printf 'look\nread the manifest\nsave /tmp/log.jsonl\nquit\n' | cargo run --bin chorus -- scripts/the-nightjar-flyer.txt
cargo run --bin reach -- scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --from-state /tmp/log.jsonl
```

Analyze a script — `reach` answers "can the player obtain card X?" statically,
over the compiled script, before play:

```sh
cargo run --bin reach -- scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --why
cargo run --bin reach -- scripts/the-nightjar-flyer.txt all --cost --by 600
cargo run --bin reach -- scripts/the-nightjar-flyer.txt demand "aspect:secret>=2"
cargo run --bin reach -- scripts/the-nightjar-flyer.txt verb WORK
cargo run --bin reach -- scripts/the-nightjar-flyer.txt world
cargo run --bin reach -- scripts/the-nightjar-flyer.txt graph   # Graphviz DOT
```

`card` prints one card's verdict and proven paths, `all` walks every name,
`demand` matches a slot query, `verb` reports one verb, `world` renders the
whole-world dashboard, and `graph` emits the mint dependency graph as DOT.
`--json` turns the card faces machine-readable; `--help` prints the full
table.

Lint a script:

```sh
cargo run --bin lint-scripts                                  # the sorted scripts/ default
cargo run --bin lint-scripts -- --reach scripts/the-nightjar-flyer.txt   # + the reach design-smell findings
```

`--color` opts every bin's text faces into ANSI highlighting (default off —
the plain output is plain bytes).

New here? Start with the tutorial: [`docs/tutorial.md`](docs/tutorial.md).
