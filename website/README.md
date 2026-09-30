# Public website

This directory is Bokhylle's one-page introduction. It explains the product,
shows the current app, and links to the source and a separate [sample demo](../demo/README.md).
The site is plain HTML, CSS, and JavaScript with locally hosted fonts. It does
not need Node.js or the Bokhylle server to run.

## Preview locally

From the repository root:

```sh
python3 -m http.server 8082 --bind 127.0.0.1 --directory website
```

Open <http://localhost:8082>. The demo buttons point to port 8081 on the same
host. Follow the [demo guide](../demo/README.md#local-preview) to start a local
demo. The source site otherwise assumes `https://demo.<main domain>` when
served from a public domain.

## Refresh the previews

`assets/demo-library.png` and `assets/demo-library-mobile.png` show the adult
view of the sample demo's household library. After building the current app and
starting the local demo, run:

```sh
pnpm -C frontend install --frozen-lockfile
node frontend/scripts/capture-website.mjs
```

The script requires Chrome and the frontend's installed dependencies. Set
`BOKHYLLE_DEMO_BASE` if your demo is not at `http://127.0.0.1:8081`. It enters
the demo as an adult and captures both screen sizes. Check the resulting images
before committing them. Do not use a household installation for website images.

`assets/demo-reader.png` shows warm reading colors and display settings;
`assets/demo-reader-mobile.png` shows the default EPUB reader. Both use the
fictional *Where Maps End*. To refresh them and
`../docs/media/reader-mobile.png`, build the frontend, start its local preview
on port 4175, then run `node frontend/scripts/capture-reader.mjs`. The script
generates a fictional EPUB and mocks the API. Never capture a household library.

The site uses the app's Paper palette and bookshelf mark. Keep
`assets/bokhylle-icon.svg` in sync with
`frontend/public/brand/bokhylle-icon.svg` when changing the brand.

The catalogue previews use fictional titles from
`node frontend/scripts/capture-docs.mjs`. With the frontend preview running,
capture `docs/media/catalogues-desktop.png` and `catalogues-mobile.png`, then
copy them to `assets/catalogues-desktop.png` and `catalogues-mobile.png`.
The public demo keeps external acquisition disabled.

The household sign-in previews use fictional profiles and illustrated portraits.
With the built frontend preview running, run:

```sh
BOKHYLLE_DOCS_SELECT=sign-in-desktop.png,sign-in-mobile.png node frontend/scripts/capture-docs.mjs
cp docs/media/sign-in-{desktop,mobile}.png website/assets/
```

The mobile image shows the selected child's PIN panel. The public demo keeps
its separate adult/child entry screen rather than requiring a password.

Metadata correction previews also use the fictional catalogue. With the built
frontend preview running, refresh them with:

```sh
BOKHYLLE_DOCS_SELECT=metadata-corrections-desktop.png,metadata-corrections-mobile.png node frontend/scripts/capture-docs.mjs
cp docs/media/metadata-corrections-{desktop,mobile}.png website/assets/
```

## Publish the static site

Build into a new directory with the actual HTTPS demo address:

```sh
python3 website/build.py --demo-url https://demo.example.com --output /tmp/bokhylle-website
```

Serve that directory through an HTTPS web server. Do not serve the repository
root; it can contain ignored local configuration and data. The
[Caddy example](../demo/Caddyfile.example) serves the website and forwards a
separate demo domain to its loopback port. The [publishing guide](../docs/publishing.md)
covers release checks and hosting.
