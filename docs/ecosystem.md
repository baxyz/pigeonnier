# Pigeonnier — the ecosystem

Written on November 27, 2026. [spec.md](spec.md) describes the GNOME app as it stands today: one
app, local, with no server. This document describes where the project is going. Nothing in it is
built yet; where the two disagree, the spec is what exists and this is the plan.

## Goal

Run your dev containers and coding agents from any of your devices, without a terminal.

Pigeonnier is two things:

- **a server** on the machine that hosts your dev containers;
- **a native app** on each device you work from, plus `pij`, a command-line client.

The question the GNOME app answers today — *what is waiting for me, project by project?* — stays.
It gains a second one: *what is running, and where?*

## Principles

- **Generic.** The tool serves anyone's machines. Nothing in the code, the defaults or the docs
  names a particular host, account or organization.
- **Public.** Every repository is free software under AGPL-3.0-or-later, and holds nothing
  confidential.
- **Optional integrations stay optional.** A secrets store, an identity server, a task source or
  a folder mounted into the containers is something a user turns on. A host with Docker and
  nothing else is enough to start.
- **One native app per platform.** Each app follows the look and the design system of its
  system. What the apps share is the core, not the interface.
- **The core does everything but draw.** Fetching, rules, filters, sorting and view-models live in
  the core; an app renders what the core gives it and sends actions back.
- **Each step ends at a gate** the maintainer checks on their own devices, as in the spec. The
  next step starts once the gate is passed.

## Parts

```text
 a device                                the host
┌───────────────────────────┐          ┌─────────────────────────────────────────────┐
│ the platform's interface   │          │ the server                                   │
│ (GTK, Compose, WinUI, …)   │          │  ├ projects    Docker + the devcontainer CLI │
│          ▲ view-models     │          │  ├ injections  what a container is handed    │
│ core + client (Rust,       │◄────────►│  ├ agents      sessions: list, state, resume │
│ local cache)               │          │  ├ host        metrics, housekeeping, alerts │
└───────────────────────────┘          │  ├ sources     GitHub and others → cache     │
 pij: the same client, in a terminal    │  ├ tasks       things to do by hand          │
                                        │  └ MCP         the same actions, for agents  │
                                        └─────────────────────────────────────────────┘
```

### The server

A single executable, written in Rust, running as an ordinary user on the host.

- **Projects.** A project is a dev container and what it works on. The server starts, stops,
  pauses, resumes and rebuilds containers, follows a build step by step, and runs a command in
  one. It talks to the Docker Engine API and leaves `devcontainer.json` to the reference
  devcontainer CLI.
- **Injections.** What a container is handed without it being written in `devcontainer.json`, so
  that the same devcontainer still works anywhere else: short-lived credentials from a secrets
  store, a git identity and commit signing through an agent on the host, settings for the coding
  agent, and **folders of the host mounted read-only or read-write**. Each injection is declared
  per host or per project, and says when it acts: before the container is created, before or
  after its lifecycle commands, on a timer, before it stops.
- **Agents.** The server lists the coding-agent sessions of a container, tells which ones are
  waiting for their user, resumes one after a restart, and opens it in the agent's own app. It
  supervises; it is not a chat client.
- **Host.** Processor, memory and disks; the space containers and images take; whether the
  services a setup relies on are installed and running; what is about to expire. The server also
  does the housekeeping: stopping containers nobody uses, cleaning stale caches.
- **Sources and tasks.** The accounts (GitHub today) are connected once, on the server, and every
  app sees the same list. Tasks to do by hand — written by the user or by an agent, with or
  without a date, filed by project — are kept by the server itself, so that no outside task
  service is needed.
- **MCP.** The server offers its actions to coding agents over MCP, each container getting access
  to its own project only.

Installing and configuring the host's services is not the apps' job: it is done on the host,
with the command line. The apps show what is installed and in what state.

### The core

`pigeonnier-core` already separates fetching, mapping, rules, filtering, sorting and view-models,
and the GNOME app reaches it through one module. Its engine becomes an interface with two
implementations: the local one of today, where the app queries the services itself, and a remote
one, where the app queries a Pigeonnier server.

On platforms whose interface is not written in Rust, the core is exposed through generated
bindings.

### The apps

| Platform | Interface | State |
| --- | --- | --- |
| GNOME | GTK 4 and libadwaita (Relm4) | Exists (see the spec) |
| Android | Jetpack Compose | Planned |
| Windows | The system's own toolkit | Planned; the toolkit is to be chosen after a trial |
| Sailfish OS | Silica | Planned; later |

### `pij`

The command-line client: the same client library as the apps, in a terminal. It is also how a
host is set up.

## Reaching the server

Two ways, chosen when an app first connects to a host:

1. **SSH.** The app reaches the server through the host's own SSH service, with a key the host
   already trusts. Nothing new listens on the network, and there is no certificate to manage: a
   host that accepts SSH is enough.
2. **[brig·id](https://brig.id).** The user signs in with a passkey on a brig·id server of their
   choice, picked at first launch. The server then enrolls the device, so that later launches do
   not need the passkey again; dangerous actions ask for it anew.

Whatever the way in:

- the server is meant for a private network; it is not built to face the Internet;
- it runs no arbitrary command on the host: its actions are the ones listed here;
- deleting a project, or granting write access to a secrets store, asks for a confirmation;
- every action is logged with the device it came from;
- the server hands out credentials without reading the secrets they open.

## Steps

| Step | Content | Gate |
| --- | --- | --- |
| **0 — The app is right** | Milestones M1 and M2 of the spec, against the real services | The list is right for a few days |
| **1 — The server, over SSH** | Projects, their state, start, stop, pause, a build followed live; the host's metrics and housekeeping; `pij`; a host page in the GNOME app | A week without a terminal to run containers |
| **2 — Identity and the phone** | Sign-in with brig·id, devices; the Android app with projects, host and alerts | Starting a container from the phone |
| **3 — Sources and tasks on the server** | Accounts held by the server; tasks; MCP | No account to connect per device; agents file their tasks in Pigeonnier |
| **4 — Agents** | Sessions listed with their state, resumed after a restart | Resuming a session from the app |
| **5 — Windows, injections, projects** | The Windows app; injections set from the apps; adding and removing a project | Adding a project without a terminal |
| **6 — Sailfish OS** | The Sailfish app | To define |

A terminal inside the apps (a shell in a container) is a goal with no step yet: it needs a
terminal widget on each platform, and waits until running a command and opening the system's
terminal are no longer enough.

## To settle by trying

- Whether the coding agent's hooks fire in sessions started remotely: live session state depends
  on it.
- The toolkit of the Windows app, on ARM64.
- The bindings and the build chain for Android.
- Whether an existing engine can carry the tasks and their offline use, or the server keeps its
  own.
