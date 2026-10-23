# Security policy

## Reporting a vulnerability

Please report security issues privately through GitHub's
[Report a vulnerability](https://github.com/alexpalexpinne/bokhylle/security/advisories/new)
flow. Do not include exploit details,
tokens, personal data, or private library content in a public issue. The
maintainer will acknowledge the report, assess impact, and coordinate a fix
and disclosure.

## Deployment boundary

Bokhylle is designed for a household server. The default Docker Compose
configuration exposes an HTTP port on the host and does not enable Secure
cookies. For access from outside a trusted local network, use an HTTPS
reverse proxy and set `BOKHYLLE_SECURE_COOKIES=true`. Set
`BOKHYLLE_TRUSTED_PROXY=true` only when the proxy is controlled and strips
untrusted forwarding headers. The unauthenticated profile picker lists
member names and roles; account passwords and reader or agent tokens still
protect access.

Password and PIN hashing and verification run outside the async request
workers, with at most two credential operations running concurrently across
the server. This limit also covers account creation and credential changes;
cancelling a request does not release capacity until its hashing finishes.

Protect the config directory and its backups: they contain the SQLite
database and can contain integration secrets. The in-app backup download
redacts secret settings, but scheduled database backups do not.

## Dependency checks

Before a release, run `cargo deny --locked check advisories licenses` and
`pnpm -C frontend audit` as well as `make check`. The functional checks do not
replace fresh dependency advisories. CI runs dependency checks separately.
