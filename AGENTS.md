# AGENTS.md — baxyz `pigeonnier`

This repository inherits the [canonical workspace rules](https://github.com/baxyz/.dev/blob/main/AGENTS.md). Only project-specific details are documented here.

## Scope

A local-first desktop companion that aggregates GitHub pull requests, GitHub Projects roadmaps and
Todoist tasks by project. [docs/spec.md](docs/spec.md) holds the design and the milestones.

**Status:** see the table in [README.md](README.md).

## License

**AGPL-3.0-or-later**, like Folioscope. Every source file starts with an SPDX header:

```rust
// SPDX-License-Identifier: AGPL-3.0-or-later
```

`cargo deny` enforces the allowed dependency licenses.

## Commit Scopes

Defined in `scopes.json`:

| Scope                                                 | Covers                                 |
| ----------------------------------------------------- | -------------------------------------- |
| `core`                                                | `crates/pigeonnier-core`               |
| `cli`                                                 | `crates/pigeonnier`, its MCP server    |
| `daemon`                                              | `crates/pigeonnierd`                   |
| `gnome`                                               | `apps/gnome`                           |
| `extension`                                           | `extensions/gnome`                     |
| `packaging`                                           | `packaging/`                           |
| `build`, `test`, `docs`, `deps`, `devcontainer`, `ci`, `ai` | As usual                         |

## Structure

```text
pigeonnier/
  crates/
    pigeonnier-core/   ← the pipeline, one module per step:
                         source/ → map → normalize → filter → sort, with view and viewmodel;
                         engine is the one interface the rest talks to
    pigeonnier/        ← the CLI and the MCP server
    pigeonnierd/       ← the Linux daemon: sync loop, D-Bus, notifications
  apps/gnome/          ← the window (Relm4, libadwaita): a D-Bus client with no logic of its own
  extensions/gnome/    ← the GNOME Shell extension (GJS); model.js has no GNOME import, so it is tested with Node
  packaging/           ← systemd, D-Bus and desktop files, install.sh
  docs/spec.md         ← the design and the milestones
```

## Commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
node --test extensions/gnome/
```

To see the window without a desktop, run it under `Xvfb` with `GDK_BACKEND=x11 GSK_RENDERER=cairo`,
next to `pigeonnierd` in a `dbus-run-session`. `PIGEONNIER_GITHUB_API` and `PIGEONNIER_TODOIST_API`
point the daemon at made-up services.

## Rules

- **Keep the steps separate.** A source only fetches, `map` only renames, `normalize` applies the
  rules of each kind of item, `filter` only selects, `sort` only orders. Mappings, predicates and
  sort keys are values, not code paths.
- **View-models live in the core.** A UI draws and sends actions; a new state or sort rule must not
  touch it.
- **Absent is not false.** A missing field makes a predicate `Unknown`, never `False`.
- **No token or personal data in the repository.** Tests use made-up payloads.
