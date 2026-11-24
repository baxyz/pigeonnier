# Pigeonnier — spec

Rewritten on November 11, 2026. The first spec described a product for three platforms with a
daemon, a CLI, a Shell extension and an MCP server; none of it could be tried. This one describes
an app.

## Goal

A GNOME app that answers one question: **what is waiting for me, project by project?**

It gathers, for each project:

- **Pull requests** (GitHub): reviews requested from me, and my open pull requests that need
  something from me (red CI, changes requested, conflicts, ready to merge).
- **Tasks** (Todoist): the tasks of the project's section, including the manual actions Claude
  asked for (label `claude`).
- **Roadmap** (GitHub Projects): the open items of the project's GitHub Project.
- **Issues** (GitHub): the open issues of the project's repositories, either those assigned to me
  (the default), all of them, or none. An issue already on the project's roadmap is listed once.

A **project** is a GitHub account or organization, or one of its repositories, optionally one of
its GitHub Projects, and optionally one Todoist section. A project made of one repository takes
that repository's pull requests and issues away from the project of its owner. An issue assigned
to me that no project lists goes to "Autres". The app proposes projects on the first run and lets every one of
them be fixed by hand.

## Principles

- **An app, and only an app.** Nothing else to install, start or keep running: no CLI, no daemon,
  no Shell extension.
- **Simple to install.** One command builds the Flatpak in the devcontainer; one command installs it
  on the host. Nothing is built on the host.
- **Everything in the window.** Accounts are connected from the app; projects are chosen from
  drop-downs of what the accounts can see. Editing a file is never required.
- **The project first.** The list is never split by service.
- **The services stay the source of truth.** The app keeps a cache, never a copy.
- **Local, no server.** Tokens stay in the system keyring.

## Not in scope

- A GitHub client or a Todoist replacement: reading a pull request happens in the browser.
- Time tracking, calendars, several users.
- Any platform other than GNOME, until asked for.

## Milestones

Each milestone ends at a **gate** that the maintainer checks on their own computer. The next
milestone starts only once the gate is passed.

| Milestone | Content | Gate |
| --- | --- | --- |
| **M1 — It installs and connects** | `packaging/build-flatpak.sh` in the devcontainer; one install command on the host. A first-run page to connect GitHub and Todoist; the settings to reconnect or disconnect. | The maintainer installs the app, opens it and connects both accounts. |
| **M2 — The list** | Pull requests, tasks, issues and roadmap items in three panes: a sidebar of saved filters, the list under its filter bar, the detail of the selected item; a home page. The settings choose and fix the projects. | The maintainer uses it for a few days and the list is right. |
| **M3 — Act** | Open, complete, snooze, quick add, merge after a confirmation. | The maintainer no longer goes to Todoist or GitHub for these. |
| **M4 — In the background** | Notifications, a counter on the dock icon, actions in the dock's right-click menu, the app staying alive with its window closed. | To define together. |

**Later, on request only:** a GNOME Shell extension (a separate, optional project, published
through extensions.gnome.org and never required by the app); an MCP server so that Claude can read
the list and add manual actions; other platforms.

**Where things stand:** the code of M1 to M3 exists and works against made-up services. The app
carries the client ID of its GitHub OAuth App. Nothing has run against the real services yet.

## Working rules

- **Actions for the maintainer go to their task list**, with the steps, rather than being buried in
  a reply: a GitHub setting, an account, a client ID to provide.
- **The build happens in the devcontainer**, never on the host.
- **The app is checked by running it**, under a virtual display against made-up services, before a
  pull request; screenshots show what changed.

## Reference

### Sources

| Source | Request | Notes |
| --- | --- | --- |
| Reviews | GraphQL search `is:pr is:open archived:false review-requested:@me` | Optionally `user-review-requested:@me` for direct requests only |
| My pull requests | GraphQL search `is:pr is:open archived:false author:@me` | Optionally the ones assigned to me |
| Issues | GraphQL search `is:issue is:open archived:false assignee:@me`, then `user:<owner>` or `repo:<repo>` for each project that lists all its issues | |
| Roadmap | `repositoryOwner(login:) { ... on ProjectV2Owner { projectV2(number:) } }`, 100 items a page | `null` without an error means the token cannot read it |
| Todoist | `POST /api/v1/sync` with the last `sync_token` | One call per refresh after the first |

The number of calls does not depend on the number of repositories: the repository of each pull
request or issue decides its project.

### State of my pull requests, most urgent first

1. Review requested → *à relire*
2. `CHANGES_REQUESTED` → *à corriger*
3. CI `FAILURE` or `ERROR` → *CI rouge*
4. `CONFLICTING` → *conflits*
5. `APPROVED`, or no review required, with green or no CI and `MERGEABLE` → *à merger*
6. Otherwise, or draft, or `mergeable` still `UNKNOWN` → *en attente*, greyed out

Pull requests from bots are listed like the others, at the lowest priority; the views of what
waits for me leave them out with the filter `-auteur:robot`.

### The window

- **Three panes.** The sidebar holds the home page and saved filters; the list shows one of them
  under its filter bar; the detail shows the selected item and its actions. A narrower window
  folds the sidebar away, then shows one pane at a time.
- **The sidebar only holds saved filters**, and the filter bar always shows the one in use. A view
  filters by state or date, a project by project; the grouping is a setting apart.
- **A filter** is a field, values and "is" or "is not". The fields: project, type, source, state,
  priority, due date, author (a person or a robot), last change. Filters on different fields
  combine; the values of one field alternate. A query can be typed (`type:pr état:ci-rouge,conflits
  -auteur:robot`), and a saved view keeps it as typed, in the configuration.
- **Grouping** has one or two levels among project, state, type, due date and priority. Groups are
  always open, and never change nature unless asked to.
- **Every row has the same anatomy:** priority, a colored icon for the type, the title over where
  the item lives, then the state and the date.
- **Triage is the normal use of the list.** After an action (open, complete, snooze, merge) the next
  item is selected. The focus mode leaves the detail alone on screen. "Commencer", on the home
  page, opens what waits for me on its first item, in focus mode: it is the only entry button.
- **The home page** shows four figures, a card per project with what presses, and the items that
  have a date; everything on it opens a list.

### Priorities and due dates

| Source | P1 | P2 | P3 | P4 |
| --- | --- | --- | --- | --- |
| Todoist (`priority`, reversed in the API) | 4 | 3 | 2 | 1 |
| Roadmap (priority field) | P0 | P1 | P2 | none |
| Pull request | label `urgent` or `priority:high` | review, red CI, changes requested | to merge, conflicts | waiting, draft, bots |

A review requested more than two working days ago moves up one step. Todoist uses `deadline`, then
`due`; the roadmap its date field, then the end of the current iteration; a pull request the due date
of its milestone.

### Sign-in

- **GitHub:** device flow with the app's OAuth App (scopes `repo`, `read:org`, `read:project`). A
  fine-grained token cannot read the Projects of several owners or of a personal account.
- **Todoist:** OAuth with PKCE and no secret, the browser coming back to `http://localhost:8745`.
  Access tokens last an hour and refresh tokens work once; a file lock keeps two refreshes apart.
- Tokens live in the keyring through the Secret Service; a keyring that does not answer within a
  minute is reported as an error.
