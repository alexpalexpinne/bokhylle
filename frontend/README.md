# Bokhylle frontend

React 19 + TypeScript + Vite + Tailwind v4 single-page app, served by the
`bokhylle-server` binary from `dist/`. Product behavior lives in the backend;
this app renders it.

The app has page components in `src/pages`, shared UI in `src/components`, and
API clients in `src/api`. See [design principles](../docs/design-principles.md)
and the [Spotlight and shelves specification](../docs/spotlight-and-shelves.md)
for the current Home and browsing behavior.

## Commands

```text
pnpm -C frontend install   # install dependencies (Node 24)
pnpm -C frontend dev       # Vite dev server (proxies /api to the backend)
pnpm -C frontend build     # production build into dist/
pnpm -C frontend lint      # oxlint
pnpm -C frontend test:e2e  # Playwright smoke suite against a live server
```

`test:e2e` checks the runner configuration, then runs browser specs against a
running server. Use a disposable database: some specs change household settings
and profiles. Build the frontend first so the server serves the current assets.

Set `BOKHYLLE_E2E_BASE` (default `http://127.0.0.1:8099`),
`BOKHYLLE_E2E_USER` (default `admin`), and `BOKHYLLE_E2E_PASSWORD`.
`BOKHYLLE_E2E_STRICT=1` fails if the server is unavailable or any selected
authenticated spec lacks credentials. Optional runs report passed, failed, and
skipped counts; skipped specs do not count as passes.

To run specific specs, set `BOKHYLLE_E2E_SPECS` to comma-separated names,
for example `onboarding-search,book-detail,profile-help`. Unknown names fail.
Specs that mock authentication can run in strict mode without a password;
they still need a server to serve the built app. Provider endpoints are mocked,
so browser checks need no internet. See the root `AGENTS.md`
for the repository conventions and the shared commands (`make check` runs lint,
type-check, tests and the production build).

## README screenshots

The root README and website use the isolated demo's selected Standard Ebooks
editions and their actual covers. Build the current frontend, start the
[local demo](../demo/README.md#local-preview), and capture Home, library, and
reader views at desktop and phone sizes:

```sh
pnpm -C frontend build
node frontend/scripts/capture-website.mjs
```

Set `BOKHYLLE_DEMO_BASE` if the local demo is not at `http://127.0.0.1:8081`.
The script requires Chrome, checks that demo mode is enabled, and updates
matching images in `docs/media/` and `website/assets/`. Reader previews use
**Paper**. Inspect the images before committing them; never capture a household
installation. See [website previews](../website/README.md#refresh-the-previews)
and the [edition rights review](../demo/RIGHTS.md).

Detailed feature illustrations still use fictional data from
`node frontend/scripts/capture-docs.mjs`, with the built frontend preview
running on port 4173. Set `BOKHYLLE_DOCS_BASE` to change that address or
`BOKHYLLE_DOCS_SELECT` to select comma-separated image filenames. These captures
leave the demo-based Home, library, and reader previews alone.
