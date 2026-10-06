# LVK builds and releases

Lucky Vibe Kanban is distributed only as Docker images. The former Windows NPX
artifact and npm publication instructions are retired.

- Develop from source using [README.md](README.md#develop-lvk).
- Validate the server configuration with `pnpm run server:check`.
- Build and validate the Docker `server` target using
  `.github/workflows/publish-server.yml` in GitHub Actions. Manual dispatch
  validates without publishing; version tags publish to GHCR after validation.
- Deploy a versioned image by immutable digest following the
  [server container guide](docs/self-hosting/server-container.mdx).

LVK does not publish npm packages. Upstream native binary names remain unchanged.
