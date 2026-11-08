# Pigeonnier

A desktop companion that answers one question: **what is waiting for me, project by project?**
It gathers pull requests and roadmap items from GitHub and manual actions from Todoist, grouped by
project rather than by service. Local-first, with no server. See [docs/spec.md](docs/spec.md).

## Status

| Part | State |
| --- | --- |
| Core, CLI, daemon, GNOME window, MCP server | Work; tested against made-up services, and GitHub pull requests against the real API |
| Roadmap and Todoist | Written from the API documentation; not yet run against the real services |
| GNOME Shell extension | Written and unit tested, not yet run in a Shell |
| First-run wizard, OAuth, quick add in the window, detail panel, search | Not done |
| Merge and re-run from the app, "Confier à Claude" | Not done |
| Windows, Android, Flatpak, `.deb` | Not done |

## Install

Needs Rust, `libadwaita-1-dev` and `libgtk-4-dev`.

```bash
packaging/install.sh
systemctl --user daemon-reload && systemctl --user enable --now pigeonnierd
gnome-extensions enable pigeonnier@baxyz.github.io   # after logging out and back in
```

## Connect

- **GitHub:** `gh auth login` with `read:project` and `read:org`, plus `repo` for private
  repositories. A fine-grained token cannot read the projects of several owners.
- **Todoist:** `pigeonnier login` reads an API token on standard input and saves it in the system
  keyring. `TODOIST_TOKEN` takes precedence.

## Configure

```bash
pigeonnier init --todoist-parent Dev           # prints a proposal
pigeonnier init --todoist-parent Dev --write   # saves it to ~/.config/pigeonnier/projects.toml
```

The daemon reloads the file when it changes. Add your own lists in
`~/.config/pigeonnier/views.json`; a view is data, in the shape of
[the default views](crates/pigeonnier-core/views/default.json).

## Use

```bash
pigeonnier list                      # all, grouped by project and most urgent first
pigeonnier list --view action        # action, today, or one of yours
pigeonnier list --ids                # with the ids the next commands take
pigeonnier done td:123               # mark a Todoist task done
pigeonnier snooze gh-pr:a/b#1 2h     # 2h, tomorrow, monday, 2026-12-01
pigeonnier add myproject "Rotate the key" --due friday --priority p2
pigeonnier-gnome                     # the window; --sidebar for the narrow layout
```

In the window: `j`/`k` move, `x` completes, `s` reports to tomorrow, `r` refreshes, `1` to `9` go to a
project. Super+S shows or hides the sidebar when the extension is on.

### For Claude

`pigeonnier mcp` serves `list_items` and `add_manual_action` over standard input and output:

```bash
claude mcp add pigeonnier -- pigeonnier mcp
```

## License

[AGPL-3.0-or-later](LICENSE).
