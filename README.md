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

The window has three panes: the sidebar, the list and the detail of the selected item.

- **The sidebar** opens the home page, then saved filters: the views (À traiter, Aujourd'hui, Mes
  reviews, Pull requests, and the ones you save) and the projects. Choosing one fills the filter bar.
- **The filter bar**, above the list, always shows what the list is filtered by. Filters combine:
  **+ Filtre** offers the project, the type, the source, the state, the priority, the due date, the
  author and the last change, each value with the number of items it would keep, and "est" or
  "n'est pas". The same can be typed: `type:pr état:ci-rouge,conflits -auteur:robot`, then Enter;
  any other word is looked for in the titles. The type buttons show a count per type. The menu on
  the right groups the list on one or two levels (project, state, type, due date, priority), orders
  it and shows what was put off. **Enregistrer la vue** adds the result to the sidebar.
- **The list** gives every item the same row: its priority, a colored icon for its type (pull
  request, issue, task, roadmap), its title over where it lives, then its state and its date. Groups
  are always open.
- **The detail** shows the description, the dates, the labels and the comments of a task, and what
  is known of a pull request, an issue or a roadmap item. Its buttons act on the item; after an
  action the next item is selected, so going down a list is handling one item after the other.
- **The home page** shows four figures, a card per project with what presses, and the few items
  that have a date. Everything on it opens a list. **Commencer** opens what waits for you on its
  first item, in focus mode.

Keys: `j`/`k` move, `Enter` or `o` opens the page, `s` snoozes to tomorrow, `x` completes a task, `m`
merges a pull request that is ready, `f` enters the focus mode (the detail alone, `Escape` leaves
it), `/` goes to the filter, `a` adds a task, `r` refreshes, `1` to `9` go to a project. Narrowed
down, the window shows one pane at a time.

## License

[AGPL-3.0-or-later](LICENSE).
