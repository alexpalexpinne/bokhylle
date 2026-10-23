# Discovery search and artwork

Search is work-level discovery. The provider supplies candidate works; Bokhylle filters unrelated candidates, ranks a bounded first pool, adds local ownership and shelf state for the current profile, and returns a cursor for the rest. A result's language describes provider metadata for a work and does not guarantee that an acquired edition or file has that language.

Provider candidates must account for the query words in the selected fields. Title searches use the core title, allow an optional leading article, and also check the provider's matching edition title, including translations. Author searches require the words in one author's name, rather than combining different contributors; `JK Rowling` and `J. K. Rowling` are equivalent. Anywhere searches can combine a title and one author (for example, `it stephen king`), or match a series or subject. A final word of at least three characters can match the start of a word while typing. Short words such as `it` never match inside `Little` or `Italian`. ISBN searches trust the provider's edition lookup because a work's returned identifiers are only a sample. Popularity, ownership and shared authorship cannot make an unrelated provider candidate eligible. The same checks apply to cached and continuation pages, and Automatic can use the fallback provider when no relevant primary candidates remain.

Anywhere searches containing only a valid ISBN use the ISBN lookup online and in the local preview. Hyphens and spaces are normalized before lookup. Ordinary numeric titles such as `1984` remain title searches.

Discover's local preview uses the same relevance checks after its catalogue lookup. Author names remain separate, including names stored as `Last, First`, so the local preview cannot combine two contributors into a match for a different person.

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
| Author `jk rowling` | The same J. K. Rowling books as the spaced initials |
| Author `brandon sanderson` | Brandon Sanderson's books, such as *The Final Empire* |
| Author `brian sanderson` | Brian Sanderson's books, distinct from Brandon Sanderson |
| Author `stephen king` | Stephen King's books before unrelated matches |
| `it` | *It* by Stephen King (short title search) |
| `it stephen king` | *It* by Stephen King before derivative titles |
| Author `ursula k le guin` | Ursula K. Le Guin before composite names |
| `the courage to be disliked` | The work by Ichirō Kishimi and Fumitake Koga, even when its work title is Japanese |

These checks passed against the discovery service with live Open Library results on 2026-10-14. In that check, `project hail mary` kept seven relevant candidates from 30 provider hits, and Author `brian sanderson` kept one from seven. Provider results change, so a live check is a signal, not a deterministic test. The first provider page requests up to 50 candidates. Every relevant ranked candidate remains reachable through `pool:` continuation tokens before the provider cursor advances. Book identity is the provider work key; identical titles with different work keys may still appear as separate results.

## Cache behavior

Discover cancels outdated local previews, online requests, and pagination when
the query, search type, language mode, or profile changes, and when leaving the
page. Clearing the query also cancels active requests. Cancelled searches do not
show failure messages or populate the result cache, and stop fetching more
provider pages. Genuine online failures still preserve an available local preview.

| Data | Freshness and failure behavior |
| --- | --- |
| Discover browser results | Five minutes, scoped by profile, query type, and language mode. Failed online searches are retryable. |
| Open Library search pages | 24 hours in SQLite; expired pages can serve during a provider failure. |
| Google Books search pages | Six hours in SQLite; the same stale fallback applies. |
| Author search | 30 days for results, one day for confirmed empty results; errors can use stale results and are not cached as misses. |
| Provider covers and portraits | Successful images are stored under `config/cache/provider-covers` and `config/cache/authors`. Confirmed missing images are retried after seven days; network errors are retryable immediately. Entries older than 90 days are pruned daily, with a shared 256 MiB cap. |

SQLite metadata rows more than 30 days past expiry are pruned on server startup. Artwork uses the server proxy so the browser does not contact the provider with a reader's IP address and browsing activity. Search cards request medium Open Library covers; the Home hero requests a large cover with eager loading and a placeholder while it arrives. Portraits use the medium variant. Cold images still depend on provider response time; in one local check a cold cover took about three seconds, while its cached repeat took about two milliseconds. Covers extracted from owned files and covers attached to stored books now live under `config/artwork/covers`. Existing `config/cache/covers` files remain readable for books that reference them; keep that legacy directory with backups until those books are refreshed.
