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

A **project** is a GitHub account or organization, optionally one of its GitHub Projects, and
optionally one Todoist section. The app proposes projects on the first run and lets every one of
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
| **M2 — The list** | Pull requests, tasks and roadmap items grouped by project; the *Tout*, *Action requise* and *Aujourd'hui* filters; search. The settings choose and fix the projects. | The maintainer uses it for a few days and the list is right. |
| **M3 — Act** | Open, complete, snooze, quick add, merge after a confirmation. | The maintainer no longer goes to Todoist or GitHub for these. |
| **M4 — In the background** | Notifications, a counter on the dock icon, actions in the dock's right-click menu, the app staying alive with its window closed. | To define together. |

**Later, on request only:** a GNOME Shell extension (a separate, optional project, published
through extensions.gnome.org and never required by the app); an MCP server so that Claude can read
the list and add manual actions; other platforms.

**Where things stand:** the code of M1 to M3 exists and works against made-up services. GitHub
cannot be connected until the app has the client ID of its GitHub OAuth App (a task for the
maintainer). Nothing has run against the real services yet.

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
| Roadmap | `repositoryOwner(login:) { ... on ProjectV2Owner { projectV2(number:) } }`, 100 items a page | `null` without an error means the token cannot read it |
| Todoist | `POST /api/v1/sync` with the last `sync_token` | One call per refresh after the first |

The number of calls does not depend on the number of repositories: the owner of each pull request
decides its project.

### State of my pull requests, most urgent first

1. Review requested → *à reviewer*
2. `CHANGES_REQUESTED` → *à corriger*
3. CI `FAILURE` or `ERROR` → *CI rouge*
4. `CONFLICTING` → *conflits*
5. `APPROVED`, or no review required, with green or no CI and `MERGEABLE` → *à merger*
6. Otherwise, or draft, or `mergeable` still `UNKNOWN` → *en attente*, greyed out

Pull requests from bots are folded into one line per project.

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
