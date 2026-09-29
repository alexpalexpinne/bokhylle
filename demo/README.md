# Public demo

The demo is a disposable Bokhylle installation with 26 sample EPUBs from
Standard Ebooks. Visitors can enter as an adult or child without creating an
account. It runs separately from a household installation and uses only
`demo/data/`, which Git ignores. **Never point it at a household's config,
library, or downloads directory.**

## What visitors can try

- Browse the shared sample library and keep a personal shelf. Each visitor gets
  a separate adult/child pair; they cannot see another visitor's changes.
- Search the samples in Discover. **Get for my shelf** shows acquisition progress
  and adds a prepared book to the shelf. No outside source is contacted.
- Read a sample EPUB in Bokhylle or download it to a reading app. Your browser
  place is saved to your disposable visitor profile.
- **Send to Demo Kindle** and **Get & Send to Kindle**
  show a delivery journey in Activity, but send no email or file to a device.
- Enter as an adult to review a prepared child request for *Black Beauty* in
  Notifications. Approval adds the sample to that child's shelf.

Visitor changes may reset at any time. Discovery is limited to the
sample catalogue, and the demo does not test real acquisition or delivery
services.

## Local preview

From the repository root, with Docker and the Compose plugin installed:

```sh
python3 demo/prepare.py
BOKHYLLE_UID="$(id -u)" BOKHYLLE_GID="$(id -g)" docker compose -f demo/compose.yaml up -d --build
```

Open <http://localhost:8081>. The first scan indexes the sample EPUBs. To
preview the [website](../website/README.md) as well, run
`python3 -m http.server 8082 --bind 127.0.0.1 --directory website` and open
<http://localhost:8082>.

For a preview on another device on a trusted LAN, copy `demo/.env.example` to
ignored `demo/.env`. Set `BOKHYLLE_DEMO_BIND` to this machine's LAN IP and
`BOKHYLLE_SECURE_COOKIES=false` for HTTP. Start Compose with
`--env-file demo/.env`; serve the website with `--bind <LAN-IP>`. The website
button then points to port 8081 on that IP. Use the HTTPS setup below for
internet access.

`prepare.py` downloads pinned editions, checks SHA-256 and EPUB structure, and
writes the marker required for demo startup. Run `python3 demo/prepare.py --verify`
to check the local files without changing them. Source EPUBs are not committed.
Standard Ebooks assesses [U.S. public-domain status](https://standardebooks.org/about/standard-ebooks-and-the-public-domain)
for the source text and artwork; rights can differ elsewhere. Read the
[catalogue rights review](RIGHTS.md) before public hosting. To change an
edition, review its contributors and artwork, then update `books.tsv` with the
new SHA-256.

## Public hosting

Host the static website on one HTTPS domain and the demo behind a reverse proxy
on another. The demo Compose port binds to loopback by default; do not expose
it directly to the internet. See the [website guide](../website/README.md) for
setting the demo URL, the [publishing guide](../docs/publishing.md) for release
checks, and the [Caddy example](Caddyfile.example) for a host-level proxy.

Copy `demo/.env.example` to ignored `demo/.env`. For the supplied Caddy setup,
set `BOKHYLLE_SECURE_COOKIES=true` and `BOKHYLLE_TRUSTED_PROXY=true`. Only trust
proxy headers when your proxy replaces client-supplied forwarding headers with
the real client IP; this keeps visitor rate limits separate. Start with:

```sh
BOKHYLLE_UID="$(id -u)" BOKHYLLE_GID="$(id -g)" docker compose --env-file demo/.env -f demo/compose.yaml up -d --build --wait
```

The app refuses demo startup without its marker, or if the database contains
regular accounts or connector credentials. Demo mode disables acquisition
connectors and background acquisition and delivery jobs. It blocks admin, OPDS,
MCP, credential, normal request, acquisition, and delivery routes. Visitors can
change only their own shelf, taste, and EPUB reading positions, try demo Get and
Send, and decide requests within their visitor pair. These actions are
rate-limited. The server caps
concurrent visitor pairs at 500 and new entries at five per IP per hour.

## Reset

Run the reset script from a scheduler. It checks the demo marker and pinned
EPUBs before stopping the container, deletes visitor state, and waits for a
healthy replacement. It leaves the sample EPUBs in place and uses `demo/.env`
when present:

```sh
demo/reset.sh
```

For a daily 04:00 reset, add
`0 4 * * * /absolute/path/to/bokhylle/demo/reset.sh >> /absolute/path/to/demo-reset.log 2>&1`
to the host's crontab. Active sessions end at reset. Back up nothing from this
disposable installation.
