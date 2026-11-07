FROM rust:1.97-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake libopus-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY crates/field-trait/Cargo.toml crates/field-trait/Cargo.toml
RUN mkdir -p src crates/field-trait/src \
    && echo 'fn main() {}' > src/main.rs \
    && touch crates/field-trait/src/lib.rs \
    && cargo build --release --locked
ARG GITHUB_RUN_NUMBER=dev
ARG GITHUB_SHA=unknown
COPY src src
COPY crates crates
COPY static static
RUN touch src/main.rs crates/field-trait/src/lib.rs \
    && cargo build --release --locked \
    && ldd target/release/radio-bot

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app radio \
    && mkdir -p /app/content /data/state /data/cache \
    && chown -R radio:radio /data
COPY --from=build /src/target/release/radio-bot /usr/local/bin/radio-bot
COPY dj_configs /app/content/dj_configs
COPY pools /app/content/pools
COPY audio_profiles /app/content/audio_profiles
ENV STATE_STORE_PATH=/data/state \
    FILE_CACHE_DIR=/data/cache
USER radio
WORKDIR /app
EXPOSE 3000
ENTRYPOINT ["radio-bot", "/app/content"]
