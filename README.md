# Pigeonnier

A GNOME app that answers one question: **what is waiting for me, project by project?** It gathers
pull requests and roadmap items from GitHub and tasks from Todoist, grouped by project rather than by
service. Local, with no server. See [docs/spec.md](docs/spec.md).

## Status

| Milestone | State |
| --- | --- |
| M1 — It installs and connects | Built and checked in a virtual display against made-up services. Never run against the real services. |
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

The window opens on a welcome page; **Configurer Pigeonnier** opens the preferences:

1. **Comptes**: **Connecter** GitHub shows a code, and **Copier et ouvrir GitHub** copies it and
   opens the page to paste it on. **Connecter** Todoist opens the browser; authorize, and that is
   all.
2. **Projets**: **Proposer mes projets** makes one project per GitHub account and organization,
   paired with the Todoist section of the same name. Open a project to change its name (Enter to
   apply), narrow it to one repository, choose its issues, its GitHub Project or its Todoist section.
   Suggestions offer the projects and links that are missing, from what the accounts see and what
   lands in "Autres"; each can be applied or ignored.
3. **Application**: the issues listed by default, which pull requests to list, when a waiting
   review becomes urgent, how often to refresh.

Every change applies at once; the list behind follows, and the services are read again when the
preferences close. Replacing or removing projects can be undone from the message that appears.
The main menu (or `Ctrl+,`) brings the preferences back later. Tokens stay in the system keyring.

## Use

In the window: `j`/`k` move, `x` completes, `s` snoozes to tomorrow, `o` opens the page, `r`
refreshes, `a` adds a task, `/` searches, `1` to `9` go to a project. A Todoist task unfolds on a
click to show its description, its dates and labels, and its comments with their files; the arrow
button opens it in Todoist. **PR**, in the GitHub section of the sidebar, shows every pull
request grouped by what it waits for, as GitHub's inbox does: à relire, à corriger, CI en échec,
conflits, prêtes à merger, then mine waiting for review and my drafts. Narrowed down, the window becomes a sidebar.

## License

[AGPL-3.0-or-later](LICENSE).
