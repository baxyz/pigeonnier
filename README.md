# Pigeonnier

A desktop companion that answers one question: **what is waiting for me, project by project?**
It gathers pull requests and roadmap items from GitHub and manual actions from Todoist, grouped by
project rather than by service.

**Status:** milestone M0, a technical spike with no UI. See [docs/spec.md](docs/spec.md) for the
design and [docs/spikes.md](docs/spikes.md) for what the spike found.

## Run the spikes

```bash
cargo run -p pigeonnier-core --example gh_prs
cargo run -p pigeonnier-core --example gh_project -- helpers4 1
TODOIST_TOKEN=… cargo run -p pigeonnier-core --example todoist_sync
```

The GitHub token comes from `gh auth token`, which returns `GH_TOKEN` when that variable is set.

## License

[AGPL-3.0-only](LICENSE).
