#!/usr/bin/env bash
# CI stages for .github/workflows/ci.yml; runs in the self-hosted runner image (server-config/ci/images/dream).
set -euo pipefail
cd "$(dirname "$0")/.."

backend_flags="--cfg tokio_unstable --cfg foundations_unstable"

case $1 in
  deps)
    yarn install --frozen-lockfile --non-interactive
    v=$(grep -A1 '^name = "wasm-bindgen"$' src/viz/wasm/Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')
    if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $v" ]]; then
      cargo install wasm-bindgen-cli --version "$v" --locked --force
    fi
    ;;
  wasm)
    (cd src/viz/wasm && just ci-build)
    ;;
  unit)
    node_modules/.bin/tsx --test $(git ls-files 'src/**/*.test.ts')
    ;;
  check)
    yarn -s run check
    ;;
  rust)
    (cd src/viz/wasm && just test)
    ;;
  e2e)
    (cd geoscript_backend && RUSTFLAGS=$backend_flags cargo build)
    tmp=$(mktemp -d)
    trap 'kill $(jobs -p) 2>/dev/null; rm -rf "$tmp"' EXIT
    geoscript_backend/target/debug/geoscript-backend --generate "$tmp/config.yml" >/dev/null
    sed -i "s|db_url: .*|db_url: \"sqlite://$tmp/db.sqlite3?mode=rwc\"|" "$tmp/config.yml"
    (cd "$tmp" && exec "$OLDPWD/geoscript_backend/target/debug/geoscript-backend" --config "$tmp/config.yml") \
      >"$tmp/backend.log" 2>&1 &
    node_modules/.bin/vite dev --port 4810 --strictPort >"$tmp/vite.log" 2>&1 &
    for url in http://localhost:5810/ http://localhost:4810/; do
      if ! timeout 120 bash -c "until curl -so /dev/null $url; do sleep 0.5; done"; then
        tail -50 "$tmp"/*.log
        exit 1
      fi
    done
    E2E_BASE=http://localhost:4810 yarn -s test:e2e || {
      tail -50 "$tmp"/*.log
      exit 1
    }
    ;;
  *)
    echo "usage: $0 deps|wasm|unit|check|rust|e2e" >&2
    exit 2
    ;;
esac
