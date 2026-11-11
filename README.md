# Pigeonnier

A GNOME app that answers one question: **what is waiting for me, project by project?** It gathers
pull requests and roadmap items from GitHub and tasks from Todoist, grouped by project rather than by
service. Local, with no server. See [docs/spec.md](docs/spec.md).

## Status

| Milestone | State |
| --- | --- |
| M1 — It installs and connects | Built and checked in a virtual display against made-up services. Connecting GitHub waits for the app's OAuth App client ID. Never run against the real services. |
| M2 — The list | Same. |
| M3 — Act | Same. |
| M4 — In the background | Not started. |

## Build and install

In the devcontainer, one command builds the Flatpak bundle:

```bash
packaging/build-flatpak.sh
```

It lands in `/workspaces/.dev/dist`, which the host sees as `~/projects/baxyz/.dev/dist`. On the
host, one command installs it, or updates it:

```bash
flatpak install --user --reinstall -y ~/projects/baxyz/.dev/dist/pigeonnier.flatpak
```

The first installation also downloads the GNOME 51 runtime from Flathub. Pigeonnier then appears
among the applications.

## First run

The window opens on a welcome page:

1. **Connecter** GitHub: the window shows a code, already copied, and opens GitHub to paste it.
2. **Connecter** Todoist: the browser opens on Todoist; authorize, and that is all.
3. Pigeonnier proposes one project per GitHub account and organization, paired with the Todoist
   section of the same name. Open a project to change its name, its GitHub Project or its Todoist
   section, then **Enregistrer**.

The settings button in the header bar brings this page back later. Tokens stay in the system
keyring.

## Use

In the window: `j`/`k` move, `x` completes, `s` snoozes to tomorrow, `r` refreshes, `a` adds a
task, `/` searches, `1` to `9` go to a project. Narrowed down, the window becomes a sidebar.

## License

[AGPL-3.0-or-later](LICENSE).
