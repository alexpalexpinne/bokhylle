# Contributing

Bokhylle is an early public release. Bug reports, reproducible integration
issues, accessibility findings, and small focused fixes are welcome. For
larger behavior or schema changes, open an issue first so the intended user
flow and migration can be discussed.

## Local setup

Use stable Rust, Node.js 24, and pnpm 10. Install frontend dependencies with
`pnpm -C frontend install --frozen-lockfile` and run `make check` before
proposing a change. Browser tests require a live server and a throwaway
database; see `AGENTS.md` and `frontend/README.md`.

Keep SQL migrations forward-only. Update API callers and docs with behavior
changes. Use synthetic test books, credentials, and email addresses. Never
include a real library, database, backup, access token, or `.env` file.
When Home or Library changes visually, refresh the [README captures](frontend/README.md#readme-screenshots)
and [website previews](website/README.md#refresh-the-previews) with fictional
or demo data.
When changing an HTTP API handler, run `make openapi` to refresh the contract
and generated frontend types. `make check` verifies route and response coverage
and rejects stale generated files. See `docs/api.md` for the current scope.
CI compares existing migrations with the target branch and checks the Rust and
frontend dependency locks for advisories and approved licenses. New dependency
licenses need review and may require updating `deny.toml` or the frontend
license check script.

## Contribution licensing

Bokhylle is licensed under [AGPL-3.0-only](LICENSE). By submitting a
contribution for inclusion in Bokhylle, you license it under the same terms.
You retain your copyright; no separate contributor agreement, copyright
assignment, or signature is required.

Submit only work you have the right to license on these terms, including any
required permission from your employer. Identify third-party material, its
origin, and its license. Dependencies keep their own licenses; see the
[third-party notices](docs/third-party/README.md).

The project does not collect additional relicensing rights. A future
proprietary license for contributed work would require permission from the
relevant rights holders.
