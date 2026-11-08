# AGENTS.md — baxyz `pigeonnier`

This repository inherits the [canonical workspace rules](https://github.com/baxyz/.dev/blob/main/AGENTS.md). Only project-specific details are documented here.

## Scope

A local-first desktop companion that aggregates GitHub pull requests, GitHub Projects roadmaps and
Todoist tasks by project. [docs/spec.md](docs/spec.md) holds the design and the milestones.

**Status:** M0 (spike, no UI). Do not start M1 without the owner's agreement.

## License

**AGPL-3.0-only**, the workspace default. Every source file starts with an SPDX header:

```rust
// SPDX-License-Identifier: AGPL-3.0-only
```

`cargo deny` enforces the allowed dependency licenses.

## Commit Scopes

Defined in `scopes.json`:

| Scope                                                | Covers                                      |
| ---------------------------------------------------- | ------------------------------------------- |
| `core`                                               | `crates/pigeonnier-core/src`                |
| `daemon`                                             | `crates/pigeonnierd`                        |
| `spike`                                              | `crates/pigeonnier-core/examples`, `docs/spikes.md` |
| `build`, `test`, `docs`, `deps`, `devcontainer`, `ci`, `ai` | As usual                             |

## Structure

```text
pigeonnier/
  crates/
    pigeonnier-core/   ← the pipeline, one module per step:
                         source/ (GitHub, Todoist) → map → filter → sort, composed by view
      examples/        ← exploration binaries of M0: gh_prs, gh_project, todoist_sync
    pigeonnierd/       ← the Linux daemon, empty for now
  docs/
    spec.md            ← the design and the milestones
    spikes.md          ← what the M0 spikes found
```

## Commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Rules

- **Keep the steps separate.** A source only fetches, `map` only renames, `filter` only selects,
  `sort` only orders. Mappings, predicates and sort keys are values, not code paths.
- **Absent is not false.** A missing field makes a predicate `Unknown`, never `False`.
- **No token or personal data in the repository.** `docs/spikes.md` records counts and field names,
  never titles of private work. Tests use made-up payloads.
