<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# Working on djotls

djotls is an early-stage Rust Djot toolkit: a parser ported from djot.js, the
`djotfmt` formatter and the `djotls` stdio language server. The code is
GPL-3.0-or-later; documentation and configuration use their own SPDX
declarations.

Read the [README](README.dj) for build and usage commands and the
[contribution guide](CONTRIBUTING.dj) for tests, documentation maintenance,
logging and releases. [The LSP guide](docs/lsp.dj) describes supported behavior
and workspace limitations. Keep these as the authoritative guides instead of
repeating their instructions here.

## Code map

- `src/parser/`: lazy parser events with inclusive UTF-8 byte positions;
  `mod.rs` documents the API and synthetic final newline.
- `src/fmt.rs`: `format(input, config)` consumes parser events through
  `FmtWriter`, including wrapping, prefixes, lists, attributes and table layout.
- `src/bin/djotfmt.rs`: formatter CLI, logging, file I/O and swap-file
  replacement. `src/cli.rs` and `docs/long_about.txt` and `docs/after_help.txt`
  define its arguments and help text.
- `src/analysis/`: immutable document analysis, half-open UTF-8 byte ranges,
  UTF-16 coordinate conversion, link resolution and edit planning. Snapshot
  storage and completion tradeoffs are documented beside `Document`.
- `src/workspace.rs`: Git discovery and disk snapshots.
- `src/lsp.rs`: protocol adapter and client capabilities, compiled into
  `src/bin/djotls.rs`. The analysis library is independent of the LSP runtime.
- `tests/fmt_test.rs`: paired formatter fixtures and output idempotency.
- `tests/parser_events_test.rs`: conformance against the pinned djot.js CLI.
- `tests/lsp_test.rs`: analysis and stdio protocol tests, plus a Linux memory
  probe.
- `tests/analysis_memory_test.rs`: isolated allocation probe; see
  [memory measurement](docs/memory.dj) for commands and historical results.
- `third_party/djot/`: upstream syntax documentation; the README links to its
  syntax reference.
- `third_party/djot.js/`: reference parser used by conformance tests, with no
  JavaScript dependency in either Rust binary.

## Editing conventions

Use standard Rust formatting and existing SPDX headers. Keep design reasons
beside the relevant implementation and link longer investigations from comments.
Maintain project guides in `.dj` format. Keep this `AGENTS.md` in Markdown for
Codex instruction discovery, and CLI help in `.txt` because Clap embeds it as
plain text. Update user-facing guides when behavior changes.
