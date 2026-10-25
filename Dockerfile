# syntax=docker/dockerfile:1
#
# Katsanto server + rustypipe-botguard in one image.
#   docker build -t katsanto .
#   docker run -d -p 3000:3000 -v katsanto-data:/data -e KATSANTO_PUBLIC_URL=https://tv.example.com katsanto

# --- web app (embedded into the server binary)
FROM node:24-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- PO token helper. Its V8 needs glibc, hence Debian rather than Alpine/musl.
FROM rust:1-bookworm AS botguard
ARG BOTGUARD_VERSION=0.1.2
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install rustypipe-botguard --version "${BOTGUARD_VERSION}" --locked --root /out

# --- server
FROM rust:1-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY --from=web /src/web/build web/build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p katsanto-server \
    && cp target/release/katsanto-server /katsanto-server

# --- runtime
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data katsanto \
    && mkdir /data && chown katsanto /data
COPY --from=botguard /out/bin/rustypipe-botguard /usr/local/bin/
COPY --from=server /katsanto-server /usr/local/bin/
ENV KATSANTO_BIND=0.0.0.0:3000 \
    KATSANTO_DATA_DIR=/data \
    KATSANTO_BOTGUARD_BIN=/usr/local/bin/rustypipe-botguard \
    RUST_LOG=info
USER katsanto
VOLUME /data
EXPOSE 3000
ENTRYPOINT ["katsanto-server"]
