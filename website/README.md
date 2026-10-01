# Public website

The hosted website is [bokhylle.com](https://bokhylle.com), with the isolated
demo at [demo.bokhylle.com](https://demo.bokhylle.com).

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

Home, household library, and EPUB reader previews use the isolated demo's
selected Standard Ebooks editions and their actual covers. The reader opens
*Dracula* in **Paper**, matching the other public screenshots. Keep the
[edition and artwork rights review](../demo/RIGHTS.md) with these assets.

Build the frontend and start the [local demo](../demo/README.md#local-preview),
then run from the repository root:

```sh
pnpm -C frontend install --frozen-lockfile
pnpm -C frontend build
node frontend/scripts/capture-website.mjs
```

The script requires Chrome and the frontend's installed dependencies. Set
`BOKHYLLE_DEMO_BASE` if the local demo is not at `http://127.0.0.1:8081`.
It checks that demo mode is enabled before entering as an adult, waits for
fonts and covers, and captures desktop and phone views. It updates both
`assets/demo-{home,library,reader}*.png` and the matching files in
`../docs/media/`, so the website and README show the same app and books.
To refresh just the readers, run `node frontend/scripts/capture-reader.mjs`
against the same demo.

Check every resulting image before committing it. Keep Paper as the canonical
public appearance. Use a local isolated demo; never point the capture at a
household installation.

The household sign-in preview uses fictional profiles and bundled profile marks.
The demo uses the same **Who’s reading?** picker with adult and child choices and
no password. The household preview uses a mocked API to show named profiles and
their chosen marks. After building the frontend, start its preview on port 4173
and run:

```sh
BOKHYLLE_DOCS_SELECT=sign-in-desktop.png,sign-in-mobile.png node frontend/scripts/capture-docs.mjs
cp docs/media/sign-in-{desktop,mobile}.png website/assets/
```

Other detailed feature illustrations in the docs also use fictional data from
`capture-docs.mjs`. That script leaves the demo-based Home, library, and reader
assets alone.

The site uses the app's Paper palette and bookshelf mark. Keep
`assets/bokhylle-icon.svg` in sync with
`frontend/public/brand/bokhylle-icon.svg` when changing the brand.

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
