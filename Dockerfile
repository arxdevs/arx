# syntax=docker/dockerfile:1.7

# ---- Build stage --------------------------------------------------------
FROM rust:1-bookworm AS builder

WORKDIR /build

# Cache the dependency graph first using cargo-chef.
RUN cargo install cargo-chef --locked --version ^0.1

COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:1-bookworm AS cacher
WORKDIR /build
RUN cargo install cargo-chef --locked --version ^0.1
COPY --from=builder /build/recipe.json recipe.json
RUN cargo chef cook --release --bin arx-server --recipe-path recipe.json

FROM rust:1-bookworm AS compile
WORKDIR /build
COPY --from=cacher /build/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY migrations/ ./migrations/
RUN cargo build --release --bin arx-server

# ---- Runtime stage ------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# - ca-certificates: HTTPS to GitHub / Docker Hub
# - wget: compose healthcheck
# - tini: signal-aware PID 1
# - git: clone repos for Git Source service builds
# - docker-cli: arx invokes `docker build` (either with an explicit Dockerfile
#   or with a Dockerfile rendered from arx's built-in stack templates piped
#   over stdin).
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
         ca-certificates wget tini git docker.io \
    && rm -rf /var/lib/apt/lists/*

COPY --from=compile /build/target/release/arx-server /usr/local/bin/arx-server

ENV ARX_CONFIG=/etc/arx/config.toml
EXPOSE 7878

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/arx-server"]
