# AGENTS.md — baxyz `pigeonnier`

This repository inherits the [canonical workspace rules](https://github.com/baxyz/.dev/blob/main/AGENTS.md). Only project-specific details are documented here.

## Scope

A GNOME app that gathers GitHub pull requests, GitHub Projects roadmaps and Todoist tasks by
project. [docs/spec.md](docs/spec.md) holds the goal, the milestones and their gates.

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
| `gnome`                                               | `apps/gnome`, the app                  |
| `packaging`                                           | `packaging/`, the Flatpak              |
| `build`, `test`, `docs`, `deps`, `devcontainer`, `ci`, `ai` | As usual                         |

## Structure

```text
pigeonnier/
  crates/
    pigeonnier-core/   ← the pipeline, one module per step:
                         source/ → map → normalize → filter → sort, with view and viewmodel;
                         engine is the one interface the app talks to
  apps/gnome/          ← the app (Relm4, libadwaita): service.rs runs the engine in the background,
                         settings.rs is the accounts and projects page
  packaging/           ← build-flatpak.sh, the desktop file, the metainfo and the icon
  docs/spec.md         ← the goal and the milestones
```

## Commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
packaging/build-flatpak.sh   # the bundle, in /workspaces/.dev/dist
```

`flatpak-builder` cannot run here (the devcontainer refuses its sandbox), so the script compiles the
app and packs it for the GNOME runtime with `flatpak build-finish`, `build-export` and
`build-bundle`. The binary only needs GTK, libadwaita and the C library, which the runtime carries in
newer versions.

To see the window without a desktop, run it under `Xvfb` with `GDK_BACKEND=x11 GSK_RENDERER=cairo`
in a `dbus-run-session`. `PIGEONNIER_GITHUB_API` and `PIGEONNIER_TODOIST_API` point it at made-up
services; `TODOIST_TOKEN` and a `gh` on the `PATH` that prints a token stand in for the accounts.

## Rules

- **An app, and only an app.** No CLI, no daemon, no Shell extension: everything happens in the
  window, and nothing has to be edited by hand.
- **Actions for the maintainer go to their task list**, with the steps, not into a reply.
- **Build in the devcontainer**, never on the host; check the app by running it before a pull
  request.
- **Keep the steps separate.** A source only fetches, `map` only renames, `normalize` applies the
  rules of each kind of item, `filter` only selects, `sort` only orders. Mappings, predicates and
  sort keys are values, not code paths.
- **View-models live in the core.** The app draws and sends actions; a new state or sort rule must
  not touch it.
- **Absent is not false.** A missing field makes a predicate `Unknown`, never `False`.
- **No token or personal data in the repository.** Tests use made-up payloads.
