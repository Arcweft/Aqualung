# syntax=docker/dockerfile:1
#
# One image recipe, grok version is an ARG. Build twice:
#   docker build --build-arg GROK_VERSION=1.0.5  -t aqualung:stable ...
#   docker build --build-arg GROK_VERSION=1.0.11 -t aqualung:alpha  ...
#
# Pin versions. https://x.ai/cli/stable and /alpha are moving pointers
# (currently 1.0.5 and 1.0.11). Do not bake GROK_CHANNEL into the image.

ARG GROK_VERSION=1.0.5

FROM rust:1-bookworm AS aqualung
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN mkdir -p /out
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release -p snorkel -p topside \
    && cp /src/target/release/snorkel /src/target/release/topside /out/

FROM debian:bookworm-slim AS grok
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
ARG GROK_VERSION
ARG TARGETARCH
RUN set -eu; \
    case "${TARGETARCH}" in \
      amd64) grok_arch=x86_64 ;; \
      arm64) grok_arch=aarch64 ;; \
      *) echo "unsupported TARGETARCH=${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    url="https://storage.googleapis.com/grok-build-public-artifacts/cli/grok-${GROK_VERSION}-linux-${grok_arch}"; \
    curl -fsSL -o /grok "$url"; \
    chmod +x /grok; \
    /grok --version

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates openssl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=aqualung /out/snorkel /out/topside /usr/local/bin/
COPY --from=grok /grok /usr/local/bin/grok
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENV GROK_HOME=/var/lib/grok
ENV GROK_LEADER_SOCKET=/var/lib/grok/leader.sock
ENV AQUALUNG_PKI=/run/aqualung/pki
EXPOSE 1943 7678
ENTRYPOINT ["/entrypoint.sh"]
