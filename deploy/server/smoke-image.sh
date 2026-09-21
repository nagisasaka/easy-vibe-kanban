#!/usr/bin/env bash
# CI-only smoke test. Creates and removes only uniquely named disposable volumes
# and a container. Never run this against an existing deployment/container name.
set -euo pipefail
image=${1:?Pass the locally built image reference}
fixture=$(mktemp -d /tmp/evk-image-smoke.XXXXXX)
name="evk-smoke-$(basename "$fixture" | tr '[:upper:]' '[:lower:]')"
cleanup() {
  docker logs "$name" 2>/dev/null || true
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker volume rm "$name-home" "$name-repos" "$name-work" >/dev/null 2>&1 || true
  # The private test certificate is disposable; keep the directory for CI logs.
}
trap cleanup EXIT
chmod 755 "$fixture"
mkdir "$fixture/tls"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -subj /CN=evk.example.test \
  -addext 'subjectAltName=DNS:evk.example.test,DNS:*.preview.example.test' \
  -keyout "$fixture/tls/privkey.pem" -out "$fixture/tls/fullchain.pem" 2>/dev/null
# Test-only credentials; production instructions use an interactive prompt.
printf '%s\n' evk-smoke-only | docker run --rm -i --entrypoint htpasswd "$image" -niB test > "$fixture/htpasswd"
chmod 644 "$fixture/tls/privkey.pem" "$fixture/htpasswd"
docker run --rm --entrypoint sh "$image" -ec '
  test "$(id -u)" = 10001
  test -x /usr/local/bin/server
  test -x /usr/local/bin/agent-process-host
  vibe-kanban-mcp --help
  node --version; pnpm --version; git --version; git lfs version
  rustc --version; cargo --version; cargo watch --version
  cargo clippy --version; cargo fmt --version
  codex --version; openwiki --help >/dev/null
  cc --version; cmake --version; python3 --version
  bash -lc "cargo --version && rustc --version && pnpm --version && codex --version"
'
docker run --rm --entrypoint node \
  -e EVK_REQUIRE_NGINX_TEST=1 \
  --mount "type=bind,src=$PWD/deploy/server,dst=/tests,readonly" \
  "$image" --test /tests/nginx.test.mjs
for suffix in home repos work; do docker volume create "$name-$suffix" >/dev/null; done
start() {
  docker run -d --name "$name" \
    -e EVK_DOMAIN=evk.example.test -e EVK_PREVIEW_DOMAIN=preview.example.test \
    --mount "type=bind,src=$fixture,dst=/run/evk-secrets,readonly" \
    -v "$name-home:/home/appuser" -v "$name-repos:/repos" -v "$name-work:/var/tmp" \
    "$image" >/dev/null
  for _ in $(seq 1 90); do
    if [[ "$(docker inspect -f '{{.State.Health.Status}}' "$name")" == healthy ]]; then return; fi
    if [[ "$(docker inspect -f '{{.State.Running}}' "$name")" != true ]]; then return 1; fi
    sleep 2
  done
  echo 'Image did not become healthy' >&2; return 1
}
start
docker exec "$name" sh -ec '
  test "$(curl -ks -o /dev/null -w "%{http_code}" --resolve evk.example.test:8443:127.0.0.1 https://evk.example.test:8443/api/info)" = 401
  curl -kfsS --user test:evk-smoke-only --resolve evk.example.test:8443:127.0.0.1 https://evk.example.test:8443/api/info >/dev/null
  touch /home/appuser/smoke-persistence /repos/smoke-persistence /var/tmp/smoke-persistence
  test -f /home/appuser/.local/share/vibe-kanban/db.v2.sqlite
'
docker stop -t 150 "$name" >/dev/null
docker rm "$name" >/dev/null
start
docker exec "$name" sh -ec 'test -f /home/appuser/smoke-persistence && test -f /repos/smoke-persistence && test -f /var/tmp/smoke-persistence'
