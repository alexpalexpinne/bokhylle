# syntax=docker/dockerfile:1

FROM node:24-bookworm-slim AS frontend
RUN npm install -g pnpm@10
WORKDIR /app/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:1.98-bookworm AS deps
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Keep this layer dependent on manifests only. Source edits should reuse the
# compiled third-party dependencies from this stage.
COPY crates/bokhylle-acquisition/Cargo.toml crates/bokhylle-acquisition/Cargo.toml
COPY crates/bokhylle-core/Cargo.toml crates/bokhylle-core/Cargo.toml
COPY crates/bokhylle-importer/Cargo.toml crates/bokhylle-importer/Cargo.toml
COPY crates/bokhylle-library/Cargo.toml crates/bokhylle-library/Cargo.toml
COPY crates/bokhylle-metadata/Cargo.toml crates/bokhylle-metadata/Cargo.toml
COPY crates/bokhylle-server/Cargo.toml crates/bokhylle-server/Cargo.toml
COPY xtask/Cargo.toml xtask/Cargo.toml
RUN set -eux; \
    for crate in crates/*; do \
        mkdir -p "$crate/src"; \
        : > "$crate/src/lib.rs"; \
    done; \
    mkdir -p crates/bokhylle-acquisition/src/bin xtask/src; \
    printf 'fn main() {}\n' > crates/bokhylle-acquisition/src/bin/rank_releases.rs; \
    printf 'fn main() {}\n' > crates/bokhylle-server/src/main.rs; \
    printf 'fn main() {}\n' > xtask/src/main.rs; \
    cargo build --locked --release -p bokhylle-server

FROM deps AS backend
COPY crates ./crates
COPY migrations ./migrations
RUN find crates -name '*.rs' -type f -exec touch {} + \
    && cargo build --locked --release -p bokhylle-server \
    && grep -qa "bokhylle server listening" target/release/bokhylle-server \
    && cp target/release/bokhylle-server /app/bokhylle-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 bokhylle \
    && useradd --system --uid 10001 --gid bokhylle --home-dir /config bokhylle \
    && mkdir -p /config /library /downloads \
    && chown -R bokhylle:bokhylle /config /library /downloads

COPY --from=backend /app/bokhylle-server /usr/local/bin/bokhylle-server
COPY --from=frontend /app/frontend/dist /app/web
COPY LICENSE /usr/share/doc/bokhylle/LICENSE

ENV BOKHYLLE_BIND_ADDR=0.0.0.0:8080 \
    BOKHYLLE_CONFIG_DIR=/config \
    BOKHYLLE_LIBRARY_DIR=/library \
    BOKHYLLE_DOWNLOADS_DIR=/downloads \
    BOKHYLLE_WEB_ROOT=/app/web

EXPOSE 8080

USER bokhylle
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["bokhylle-server", "healthcheck"]

ENTRYPOINT ["bokhylle-server"]
