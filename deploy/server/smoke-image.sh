#!/usr/bin/env bash
# CI-only smoke test. Creates and removes only uniquely named disposable volumes
# and a container. Never run this against an existing deployment/container name.
set -euo pipefail
image=${1:?Pass the locally built image reference}
fixture=$(mktemp -d /tmp/lvk-image-smoke.XXXXXX)
name="lvk-smoke-$(basename "$fixture" | tr '[:upper:]' '[:lower:]')"
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
  -subj /CN=lvk.example.test \
  -addext 'subjectAltName=DNS:lvk.example.test,DNS:*.preview.example.test,IP:127.0.0.1' \
  -keyout "$fixture/tls/privkey.pem" -out "$fixture/tls/fullchain.pem" 2>/dev/null
# Test-only credentials; production instructions use an interactive prompt.
printf '%s\n' lvk-smoke-only | docker run --rm -i --entrypoint htpasswd "$image" -niB test > "$fixture/htpasswd"
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
  -e LVK_REQUIRE_NGINX_TEST=1 \
  --mount "type=bind,src=$PWD/deploy/server,dst=/tests,readonly" \
  "$image" --test /tests/nginx.test.mjs
for suffix in home repos work; do docker volume create "$name-$suffix" >/dev/null; done
start() {
  docker run -d --name "$name" \
    --shm-size=256m \
    -e LVK_HOST="${1:-lvk.example.test}" \
    --mount "type=bind,src=$fixture,dst=/run/lvk-secrets,readonly" \
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
docker exec -e LVK_E2E_BASE_URL=http://127.0.0.1:3000 "$name" node /opt/lvk-browser/smoke.mjs
docker cp deploy/developer/test-queue-fence.py "$name:/tmp/test-queue-fence.py"
docker exec --user root "$name" python3 /tmp/test-queue-fence.py
docker exec "$name" sh -ec '
  test "$(curl -ks -o /dev/null -w "%{http_code}" --resolve lvk.example.test:8443:127.0.0.1 https://lvk.example.test:8443/api/info)" = 401
  curl -kfsS --user test:lvk-smoke-only --resolve lvk.example.test:8443:127.0.0.1 https://lvk.example.test:8443/api/info >/dev/null
  touch /home/appuser/smoke-persistence /repos/smoke-persistence /var/tmp/smoke-persistence
  test -f /home/appuser/.local/share/vibe-kanban/db.v2.sqlite
'
# Exercise the resource protocol against this disposable database. Codex auth
# is intentionally absent; deterministic ownership must work without a model.
docker cp scripts/test-resource-coordination.py "$name:/tmp/test-resource-coordination.py"
docker exec "$name" python3 /tmp/test-resource-coordination.py http://127.0.0.1:3000
docker stop -t 150 "$name" >/dev/null
docker rm "$name" >/dev/null
start 127.0.0.1
docker exec "$name" sh -ec 'test -f /home/appuser/smoke-persistence && test -f /repos/smoke-persistence && test -f /var/tmp/smoke-persistence'
# Verify IP SANs with normal TLS verification, then replace the pair just as an
# issuer would. The running supervisor must reload nginx without recreating LVK.
docker exec "$name" curl -fsS --cacert /run/lvk-secrets/tls/fullchain.pem \
  --user test:lvk-smoke-only https://127.0.0.1:8443/api/info >/dev/null
started=$(docker inspect -f '{{.State.StartedAt}}' "$name")
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -set_serial 42 \
  -subj /CN=ip-renewal -addext 'subjectAltName=IP:127.0.0.1' \
  -keyout "$fixture/tls/new-key.pem" -out "$fixture/tls/new-cert.pem" 2>/dev/null
chmod 644 "$fixture/tls/new-key.pem"
mv "$fixture/tls/new-key.pem" "$fixture/tls/privkey.pem"
mv "$fixture/tls/new-cert.pem" "$fixture/tls/fullchain.pem"
renewed=false
for _ in $(seq 1 30); do
  if docker exec "$name" curl -fsS --cacert /run/lvk-secrets/tls/fullchain.pem \
    --user test:lvk-smoke-only https://127.0.0.1:8443/api/info >/dev/null 2>&1; then
    renewed=true
    break
  fi
  sleep 2
done
test "$renewed" = true
test "$(docker inspect -f '{{.State.StartedAt}}' "$name")" = "$started"
