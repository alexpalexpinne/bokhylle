# Discovery search and artwork

Search is work-level discovery. The provider supplies candidate works; Bokhylle ranks a bounded first pool, adds local ownership and shelf state for the current profile, and returns a cursor for the rest. A result's language describes provider metadata for a work and does not guarantee that an acquired edition or file has that language.

## Relevance checks

The search tests use deterministic provider fixtures for exact titles, series, companions, author evidence, pagination, and provider failures. Before changing ranking, check real Open Library results for these queries and confirm the expected first result:

| Query | Expected first result |
| --- | --- |
| `dune` | *Dune* by Frank Herbert |
| `a game of thrones` | *A Game of Thrones* by George R. R. Martin |
| `the expanse` | *Leviathan Wakes* by James S. A. Corey |
| `the lord of the rings` | *The Lord of the Rings* by J. R. R. Tolkien |
| `foundation` | *Foundation* by Isaac Asimov |
| `project hail mary` | *Project Hail Mary* by Andy Weir |
| `harry potter` | Original Harry Potter novels before collections and companions |
| Author `j k rowling` | J. K. Rowling and her books before unrelated matches |
| Author `brandon sanderson` | Brandon Sanderson's books, such as *The Final Empire* |
| Author `brian sanderson` | Brian Sanderson's books, distinct from Brandon Sanderson |
| Author `stephen king` | Stephen King's books before unrelated matches |
| `it` | *It* by Stephen King (short title search) |
| `it stephen king` | *It* by Stephen King before derivative titles |
| Author `ursula k le guin` | Ursula K. Le Guin before composite names |

These checks passed against the local preview on 2026-09-24. Provider results change, so a live check is a signal, not a deterministic test. The first provider page requests up to 50 candidates. Every ranked candidate remains reachable through `pool:` continuation tokens before the provider cursor advances. Book identity is the provider work key; identical titles with different work keys may still appear as separate results.

## Cache behavior

| Data | Freshness and failure behavior |
| --- | --- |
| Discover browser results | Five minutes, scoped by profile, query type, and language mode. Failed online searches are retryable. |
| Open Library search pages | 24 hours in SQLite; expired pages can serve during a provider failure. |
| Google Books search pages | Six hours in SQLite; the same stale fallback applies. |
| Author search | 30 days for results, one day for confirmed empty results; errors can use stale results and are not cached as misses. |
| Provider covers and portraits | Successful images are stored under `config/cache/provider-covers` and `config/cache/authors`. Confirmed missing images are retried after seven days; network errors are retryable immediately. Entries older than 90 days are pruned daily, with a shared 256 MiB cap. |

SQLite metadata rows more than 30 days past expiry are pruned on server startup. Artwork uses the server proxy so the browser does not contact the provider with a reader's IP address and browsing activity. Search cards request medium Open Library covers; the Home hero requests a large cover with eager loading and a placeholder while it arrives. Portraits use the medium variant. Cold images still depend on provider response time; in one local check a cold cover took about three seconds, while its cached repeat took about two milliseconds. Covers extracted from owned files and covers attached to stored books now live under `config/artwork/covers`. Existing `config/cache/covers` files remain readable for books that reference them; keep that legacy directory with backups until those books are refreshed.
