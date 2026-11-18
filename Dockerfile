FROM rust:1.100-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY crates/field-trait/Cargo.toml crates/field-trait/Cargo.toml
COPY crates/js-bundler crates/js-bundler
# Build dependencies against stub sources so this layer is reused until Cargo.toml or Cargo.lock changes.
RUN mkdir -p src crates/field-trait/src portal/static portal/bundle \
    && echo 'fn main() {}' > src/main.rs \
    && touch crates/field-trait/src/lib.rs portal/static/theme.js portal/static/alpine.min.js portal/static/base.css portal/static/index.css portal/bundle/index.js \
    && echo '"/static/index.js" "/static/theme.js" "/static/alpine.min.js" "/static/base.css" "/static/index.css"' > portal/bundle/index.html \
    && cargo build --release --locked
ARG GITHUB_RUN_NUMBER=dev
ARG GITHUB_SHA=unknown
COPY src src
COPY crates crates
COPY portal portal
RUN touch src/main.rs crates/field-trait/src/lib.rs \
    && cargo build --release --locked

# The binary links glibc dynamically, so the runtime image needs a glibc base. TLS uses bundled root certificates, so no CA store is installed.
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home-dir /app radio \
    && mkdir -p /data/content /data/state /data/cache \
    && chown -R radio:radio /data
COPY --from=build /src/target/release/radio-bot /usr/local/bin/radio-bot
ENV STATE_STORE_PATH=/data/state \
    FILE_CACHE_DIR=/data/cache
USER radio
WORKDIR /app
EXPOSE 3000
ENTRYPOINT ["radio-bot", "/data/content"]
