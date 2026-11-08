# M0 spikes

Results of the exploration binaries in `crates/pigeonnier-core/examples`, run on 2026-11-08 from the
baxyz devcontainer. This file records counts and field names only, never the titles of the work.

## Verdict

| Spike | Status | Evidence |
| --- | --- | --- |
| PR searches (`gh_prs`) | **Works** | Real GitHub API, all spec fields valid; small sample (at most 3 open PRs, 2 owners) |
| Project v2 items (`gh_project`) | **Query valid, items not read** | The token may not read the project; schema and cost are confirmed, the data is not |
| Todoist sync (`todoist_sync`) | **Not run against Todoist** | `TODOIST_TOKEN` is not set; only exercised against a local stand-in |
| Todoist OAuth | **Documentation only** | PKCE is confirmed by Todoist's server metadata; redirects were not tried |

To finish the two open spikes:

```bash
# Needs a token that may read the project (see "Token reach")
GH_TOKEN=… cargo run -p pigeonnier-core --example gh_project -- helpers4 1
# TODOIST_WAIT_SECS leaves time to change a task by hand before the incremental sync
TODOIST_TOKEN=… TODOIST_WAIT_SECS=60 cargo run -p pigeonnier-core --example todoist_sync
```

## GraphQL cost

Measured with `rateLimit { cost }` in the same query. The project query costs the same whether the
project is unreadable or does not exist, which fits a cost computed from the query before it runs.

| Query | Points | Requests |
| --- | --- | --- |
| One PR search, spec fields, `first: 100` | 1 | 1 |
| Both searches as aliases of one request | 2 | 1 |
| One PR search with the extra fields below | 3 | 1 |
| One project page, 100 items | 2 | 1 |

**Budget:** a refresh with the extra fields and 5 projects of at most 100 items costs 2 × 3 + 5 × 2 =
16 points. Every 5 minutes that is 192 points per hour, under 4 % of the 5,000 allowed. Each further
100 items of a project add 2 points.

Aliasing the searches saves a request, not points. The spec says both "4 calls" and "5 to 10 calls"
per refresh: the real number is 2 searches + 1 per project page + 1 Todoist sync.

## Token reach

This is the finding most likely to change the design.

- The devcontainer's `GH_TOKEN` is a fine-grained PAT, and `gh auth token` returns it instead of any
  stored login. So "use the GitHub CLI account" does not always mean a broad OAuth token.
- With that PAT, `viewer.organizations` returns **zero** organizations and no error, so the spec's
  discovery step 1 would silently find nothing.
- `projectV2(number: 1)` on helpers4 returns `null` with **no error**, while numbers 2, 3 and 99 return
  `NOT_FOUND`. `projectsV2.totalCount` is 1. So project 1 very probably exists, but its title and items
  could not be read. brig-id and tuxery behave the same.
- `ProjectRead` keeps the three outcomes apart, and the UI must say "no permission" for the `null` case
  rather than showing an empty roadmap.
- A fine-grained PAT has a single resource owner, so the spec's plan B (paste a fine-grained PAT) cannot
  cover several organizations' projects. Plan B should say a classic token, or be dropped. Searches may
  also miss private PRs of other owners without saying so; this sample had none to check with.

## Fields

All the fields the spec lists exist on `PullRequest`, including `statusCheckRollup`.

**Missing from the spec's list, needed by its own rules** (`PrFields::Extended`, 2 extra points per search):

| Rule | Field |
| --- | --- |
| Group bot PRs | `author { login __typename }`; `__typename` is `Bot` for a bot |
| A review waiting over 2 working days moves up | `timelineItems(itemTypes: [REVIEW_REQUESTED_EVENT]) { … createdAt }` |
| Tell a team request from a personal one | `reviewRequests { nodes { requestedReviewer { __typename } } }` |

**Surprising:**

- **`reviewDecision` was `null` on every PR seen.** It stays empty on repositories that do not require
  reviews. The spec's "to merge" state needs `APPROVED`, so those PRs would never reach it and would stay
  "waiting" forever. **Decided for M1:** on those repositories, my own PR with green CI and no conflict
  shows as "to merge", with no approval needed.
- **`mergeable` is computed lazily.** The only PR in the review search read `UNKNOWN` on one query and `MERGEABLE` on a later
  one. `UNKNOWN` must keep the previous state: it is neither "conflicts" nor "to merge", and must not
  trigger a notification.
- `milestone` was `null` everywhere, so most PRs have no due date. They go last in the sort and are left
  out of "overdue", through `Truth::Unknown`.
- `repositoryOwner(login:) { ... on ProjectV2Owner { projectV2(number:) } }` serves organizations and
  users with one query. The spec's split between `organization` and `user` is not needed.
- Not observed, to check with real data: `statusCheckRollup` should be `null` on a repository with no
  checks, which "CI green" must handle.

**Project items, from the schema only:**

- An iteration gives `startDate` and `duration`, not an end date: the deadline has to be computed.
- Past iterations are in `completedIterations`, apart from `iterations`.
- A draft issue has no `state`, so "open content" is `Unknown` for it. It probably needs its own rule.
- Field names are user text and end up in JSON pointers: M1 must escape `/` and `~`.

## Todoist sync

From the [official API documentation](https://developer.todoist.com/api/v1/), not from a live run:

- `POST /api/v1/sync` with `sync_token` (`*` for a full sync) and `resource_types`, as in the spec.
- `content`, `description`, `priority`, `due`, `deadline`, `labels`, `checked`, `section_id`, `is_deleted`
  and `updated_at` are documented. `priority` is indeed inverted: 4 is what the apps show as p1.
- **Limits:** 1,000 partial syncs **and 100 full syncs** per user per 15 minutes. The spec only mentions
  the first, so a lost `sync_token` must not cause repeated full syncs.
- `full_sync_date_utc` warns that a full sync of a big account can lag, so an incremental sync should
  follow it right away, as the binary does.
- An unknown token gets HTTP 401 (`AUTH_INVALID_TOKEN`) and a request with no token gets 403. Both come
  from the real API; this is the only part that ran against it.

Against the local stand-in, the binary sends the expected form, stores the token and sends it back, and
applies a deletion. `SyncState` has unit tests for this. The stand-in's shape was written from the
documentation, so this proves the plumbing, not the API.

## Todoist OAuth

| Question | Answer | Confidence |
| --- | --- | --- |
| PKCE? | **Yes, S256**, for public clients with `token_endpoint_auth_method: none` | Server metadata and documentation |
| No client secret? | **Yes**, through dynamic registration or a client ID metadata document | Documentation |
| PKCE for apps made in the App Management Console? | Not stated: that flow is documented with a secret | Unknown |
| `http://localhost` redirect? | "Must be HTTPS (localhost is allowed for testing)" | Documentation; the wording promises little |
| `http://127.0.0.1`, as in the spec? | Not mentioned | Unknown, not tried |
| `pigeonnier://callback`, as in the spec for Android? | Not mentioned, and "must be HTTPS" argues against it | Probably refused, not tried |

**Also new compared with the spec:** access tokens of new apps expire after **one hour**, and refresh
tokens rotate on each use. Reusing a consumed refresh token more than 60 seconds later **revokes all the
user's tokens for the app**. So one process must own the refresh, and each device needs its own grant.
The documentation gives `app.todoist.com/oauth/authorize` while the
[server metadata](https://api.todoist.com/.well-known/oauth-authorization-server) gives
`api.todoist.com/oauth/authorize`.

**Conclusion:**

1. **M1 and M2: paste the personal API token.** The tool is strictly personal, there is nothing to
   refresh, and it matches the spec's plan B.
2. **Later, on desktop: a public client with PKCE and a `http://localhost:<port>/callback` redirect.** No
   secret to embed, which removes the spec's main worry. The client ID metadata document is meant for
   desktop apps and only needs a static JSON file over HTTPS, so it keeps the "no backend" principle.
3. **Android: a pasted token**, unless a verified HTTPS app link is worth hosting.

**Before relying on point 2:** register one public client with a `localhost` redirect and go through the
authorization once. This was not done, because it creates a client on Todoist's side.

## Code

The core splits into `source` → `map` → `filter` → `sort`, composed by `view`. All three binaries run
their results through it, and the split held up: nothing had to cross a step.

Queries are plain strings over `serde_json::Value` for the spike. The spec's `cynic` or `graphql_client`
remains a choice for M1. `Predicate::Before` compares text, which ignores time zones: M1 must decide
what "today" means for a Todoist floating date against a UTC milestone date.
