# syntax=docker/dockerfile:1.6

FROM node:24-alpine AS fe-builder

ARG POSTHOG_API_KEY=""
ARG POSTHOG_API_ENDPOINT=""

WORKDIR /app

ENV PNPM_HOME=/pnpm
ENV PATH=${PNPM_HOME}:${PATH}
ENV VITE_PUBLIC_POSTHOG_KEY=${POSTHOG_API_KEY}
ENV VITE_PUBLIC_POSTHOG_HOST=${POSTHOG_API_ENDPOINT}
ENV NODE_OPTIONS=--max-old-space-size=4096

RUN corepack enable
RUN pnpm config set store-dir /pnpm/store

COPY pnpm-lock.yaml pnpm-workspace.yaml package.json ./
COPY .npmrc ./
COPY patches/ patches/
COPY packages/local-web/package.json packages/local-web/package.json
COPY packages/ui/package.json packages/ui/package.json
COPY packages/web-core/package.json packages/web-core/package.json

RUN --mount=type=cache,id=pnpm,target=/pnpm/store \
    pnpm install --frozen-lockfile

COPY packages/local-web/ packages/local-web/
COPY packages/public/ packages/public/
COPY packages/ui/ packages/ui/
COPY packages/web-core/ packages/web-core/
COPY shared/ shared/

RUN pnpm -C packages/local-web build

FROM rust:1.93-slim-bookworm AS builder

ARG POSTHOG_API_KEY=""
ARG POSTHOG_API_ENDPOINT=""
ARG SENTRY_DSN=""
ARG VK_SHARED_API_BASE=""

ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
ENV CARGO_TARGET_DIR=/app/target
ENV CARGO_BUILD_JOBS=2
ENV POSTHOG_API_KEY=${POSTHOG_API_KEY}
ENV POSTHOG_API_ENDPOINT=${POSTHOG_API_ENDPOINT}
ENV SENTRY_DSN=${SENTRY_DSN}
ENV VK_SHARED_API_BASE=${VK_SHARED_API_BASE}

WORKDIR /app

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    git \
    libclang-dev \
    libssl-dev \
    pkg-config \
    cmake \
    nasm \
    perl \
  && rm -rf /var/lib/apt/lists/*

COPY rust-toolchain.toml ./
RUN cargo --version >/dev/null

COPY Cargo.toml Cargo.lock ./
# Keep workspace membership and offline SQLx metadata in sync with Cargo.toml.
# The old hand-maintained list included the removed server-info crate.
COPY crates/ crates/
COPY assets/ assets/
COPY --from=fe-builder /app/packages/local-web/dist packages/local-web/dist

RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=workspace-target,target=/app/target \
    cargo build --locked --release --bin server --bin agent-process-host --bin vibe-kanban-mcp \
 && cp /app/target/release/server /usr/local/bin/server \
 && cp /app/target/release/agent-process-host /usr/local/bin/agent-process-host \
 && cp /app/target/release/vibe-kanban-mcp /usr/local/bin/vibe-kanban-mcp

FROM node:22-bookworm-slim AS runtime

# The adapter and image read the same version pin. Installation of the Codex
# host integration happens at runtime, against the actual user's mounted home.
COPY assets/openwiki-version /usr/local/share/evk/openwiki-version
ENV OPENWIKI_TELEMETRY_DISABLED=1
RUN npm install --global --omit=dev "openwiki@$(tr -d '\n' </usr/local/share/evk/openwiki-version)" \
 && node -e 'const p=require("node:fs").readFileSync("/usr/local/share/evk/openwiki-version","utf8").trim();const r=require("node:child_process").spawnSync("openwiki",["--help"],{encoding:"utf8"});if(r.status!==0||r.stdout.match(/OpenWiki v(\S+)/)?.[1]!==p)process.exit(1)'

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    openssh-client \
    tini \
    wget \
  && rm -rf /var/lib/apt/lists/* \
  && groupadd --gid 10001 appuser \
  && useradd --create-home --uid 10001 --gid 10001 --shell /bin/bash appuser

WORKDIR /repos

COPY --from=builder /usr/local/bin/server /usr/local/bin/server
COPY --from=builder /usr/local/bin/agent-process-host /usr/local/bin/agent-process-host
COPY --from=builder /usr/local/bin/vibe-kanban-mcp /usr/local/bin/vibe-kanban-mcp

RUN mkdir -p /repos \
  && chown -R appuser:appuser /repos

USER appuser

ENV HOST=0.0.0.0
ENV PORT=3000

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/bin/sh", "-c", "wget --spider -q http://127.0.0.1:${PORT:-3000}/health"]

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/server"]

# Server distribution: prebuilt EVK plus a reusable development toolchain.
# The default final image is this target; --target runtime retains the small,
# unauthenticated local image and must not be published directly to the Internet.
FROM runtime AS server
USER root
ARG CODEX_VERSION=0.154.0
ARG CARGO_WATCH_VERSION=8.5.3
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
    nginx apache2-utils openssl curl bash git git-lfs gh ripgrep \
    build-essential libclang-dev libssl-dev pkg-config cmake ninja-build nasm \
    python3 python3-venv unzip zip \
 && rm -rf /var/lib/apt/lists/* \
 && npm install --global --omit=dev "@openai/codex@${CODEX_VERSION}" pnpm@10.13.1 \
 && codex --version

# The release builder already resolved the repository's pinned Rust toolchain.
# Keep toolchains in the image, but registry/git/build caches in mounted storage.
COPY --from=builder /usr/local/rustup /opt/rustup
# Login shells may reset PATH: keep rustup proxies on the standard system PATH.
COPY --from=builder /usr/local/cargo/bin /usr/local/bin
COPY rust-toolchain.toml /usr/local/share/evk/rust-toolchain.toml
ENV RUSTUP_HOME=/opt/rustup
ENV PATH=/home/appuser/.cargo/bin:${PATH}
RUN rustup default "$(sed -n 's/^channel = "\(.*\)"/\1/p' /usr/local/share/evk/rust-toolchain.toml)" \
 && rustup component add clippy rustfmt \
 && cargo install cargo-watch --version "${CARGO_WATCH_VERSION}" --locked --root /opt/cargo-tools \
 && cp /opt/cargo-tools/bin/cargo-watch /usr/local/bin/cargo-watch \
 && rm -rf /root/.cargo/registry /root/.cargo/git /opt/cargo-tools \
 && chown -R appuser:appuser /opt/rustup \
 && chmod 1777 /var/tmp

COPY deploy/server/server.mjs deploy/server/healthcheck.mjs deploy/server/nginx.conf.template /opt/evk-server/
USER appuser
ENV HOME=/home/appuser
ENV CARGO_HOME=/home/appuser/.cargo
ENV HOST=127.0.0.1
# Do not force every project/dev script to use the control plane's PORT.
ENV PORT=""
ENV BACKEND_PORT=3000
ENV PREVIEW_PROXY_PORT=3001
EXPOSE 8443
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
  CMD ["node", "/opt/evk-server/healthcheck.mjs"]
ENTRYPOINT ["/usr/bin/tini", "--", "node", "/opt/evk-server/server.mjs"]
