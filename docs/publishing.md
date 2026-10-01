# Maintainer publishing guide

The source repository is public. This guide covers release checks and optional
image, website, and demo publishing. A normal household installation starts
with the [root README](../README.md#start-with-docker-compose).

The public project has three separate pieces:

| Piece | Location | What people get |
| --- | --- | --- |
| Source | `github.com/alexpalexpinne/bokhylle` | Code, documentation, issues, and releases |
| Docker image, when released | `ghcr.io/alexpalexpinne/bokhylle` | An installable server with the built web app |
| Website and demo | [bokhylle.com](https://bokhylle.com) and [demo.bokhylle.com](https://demo.bokhylle.com) | The static introduction and an isolated preview |

The website does not need an npm package. The React app ships inside the server
image. People can also clone the source and build through Compose without
waiting for a registry image.

Keep published SQL migrations unchanged and add new migrations for schema
changes. Review diffs for credentials, local data, database files, and build
output before pushing public changes.

## Release checks

Before publishing, run in the source tree with Node.js 24 and pnpm 10:

```sh
pnpm -C frontend install --frozen-lockfile
make check
cargo deny --locked check advisories licenses
pnpm -C frontend audit --audit-level high
pnpm -C frontend licenses:check
python3 demo/prepare.py --verify
docker compose -f demo/compose.yaml config -q
docker build -f docker/Dockerfile -t bokhylle:release-check .
python3 scripts/check_image.py bokhylle:release-check
```

If the demo samples are not downloaded yet, run `python3 demo/prepare.py`
first. The EPUBs remain local and are not part of the source repository.

Require green CI on the exact commit to be released. CI verifies fresh
installation, import, restart, and database/library restore in the
Docker image, runs strict Chromium tests, checks dependency policy, and verifies
the pinned demo catalogue. The image check compares every applied migration's
version, successful status, and checksum with the source migrations. Also check
a fresh install, update and restore on a disposable deployment. Before claiming device support has been validated,
perform an actual Kindle delivery and a Safari/iPhone check. The demo simulates
delivery and does not establish real-device success.

Keep the public-beta label and known limits visible. Enable
[private vulnerability reporting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository)
and subscribe to security alerts. Existing issue templates provide the bug
report route.

## Publish Docker images

The existing `Publish image` workflow runs for `v*` tags or manual dispatch.
After approval, push the chosen release tag or dispatch that workflow. It runs
CI, builds native amd64 and arm64 images, and smoke tests each before publishing
their combined tags. Pre-release versions do not update `latest`; operators
should pin the release version they intend to use.

Only the highest stable `vMAJOR.MINOR.PATCH` tag updates `latest`; older stable
backports leave it in place. A manual dispatch publishes a commit SHA tag only.
The workflow's architecture-specific `build-*` tags are staging artifacts,
not install tags.

GitHub's Container registry defaults newly published packages to private.
After the first publish, make the `bokhylle` package public and verify a pull
without registry credentials. See
[GitHub's registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry).
Users can select a published image through `compose.image.yaml`; the
[operations guide](operations.md#published-image) gives the pull and start commands.

## Publish the website and demo

1. Point the chosen website and demo domains to the server. Build and copy the
   static website as described in [the website guide](../website/README.md).
2. Start the isolated demo with [the demo instructions](../demo/README.md).
   Keep its Docker port on loopback. Use HTTPS, secure cookies, and
   `BOKHYLLE_TRUSTED_PROXY=true` only behind a proxy that overwrites client IP
   headers. The [Caddy example](../demo/Caddyfile.example) covers a proxy running
   directly on the host, without a CDN in front of it.
3. Verify the deployed catalogue matches [the reviewed editions](../demo/RIGHTS.md)
   and the hosting jurisdiction is covered by that review.
4. Schedule the daily demo reset. Check the reset log and that the replacement
   container becomes healthy; visitor changes are deliberately disposable.
5. Monitor `https://<demo-domain>/api/health` and the website's HTTP status.
   Check sign-in, Get & Send, a file download, and the website links over the
   public HTTPS addresses. Check that distinct visitors get distinct shelves.

### Cloudflare Tunnel on a private server

An outbound Cloudflare Tunnel can serve both domains when the host does not
accept public HTTP or HTTPS connections. Keep the demo and web proxy ports on
loopback, and run a separate `cloudflared` connector on their Docker network.
Pin all container images and mount the connector token from a restricted file;
the server does not need the Cloudflare management API token. Revoking that API
token does not revoke the connector's separate tunnel credential.

Configure one tunnel ingress rule per hostname, pointing to the web proxy, and
end with an `http_status:404` rule. Set each rule's `httpHostHeader` to its public
hostname. Create proxied CNAME records for the website, `www`, and demo pointing
to `<tunnel-id>.cfargotunnel.com`. Verify that the zone is **active** and the
registrar delegates to its assigned Cloudflare nameservers; records in a pending
zone do not switch public traffic. Preserve mail and other unrelated records,
and save the previous DNS records before replacing any parking records.

Cloudflare terminates public HTTPS and carries origin traffic through the
encrypted tunnel. The origin proxy may use HTTP on the private Docker network.
For a Caddy origin, use explicit `http://` site addresses and redirect requests
whose Cloudflare `X-Forwarded-Proto` header is `http` to the public HTTPS URL.
In the demo's `reverse_proxy` block, overwrite the application headers:

```caddyfile
header_up X-Forwarded-For {http.request.header.CF-Connecting-IP}
header_up X-Real-IP {http.request.header.CF-Connecting-IP}
header_up X-Forwarded-Proto https
header_up -Forwarded
```

Trust these Cloudflare headers only when the origin is reachable through the
controlled tunnel. This preserves per-visitor demo limits while
`BOKHYLLE_SECURE_COOKIES=true` and `BOKHYLLE_TRUSTED_PROXY=true` remain enabled.
Check connector readiness at its private `/ready` metrics endpoint as well as
public HTTPS health, cookies, redirects, and visitor isolation. Keep the daily
reset scoped to the demo service so the website and connector stay online.

See Cloudflare's [Tunnel setup](https://developers.cloudflare.com/tunnel/get-started/)
and [HTTP headers](https://developers.cloudflare.com/fundamentals/reference/http-headers/)
documentation for the current configuration and header behavior.

Publishing or changing the deployed website, demo, image, DNS, or repository
settings requires maintainer approval.
