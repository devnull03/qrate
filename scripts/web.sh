#!/usr/bin/env bash
# Builds the browser bundle into dist/web and, with --serve, serves it locally.
# gpui's web platform runs background work on Web Workers sharing one wasm memory, which needs
# nightly (wasm_thread uses #![feature], and std must be rebuilt with atomics) and a page served
# cross-origin isolated (COOP/COEP), or SharedArrayBuffer does not exist.
set -euo pipefail
cd "$(dirname "$0")/.."

profile=dev
serve=false
for arg in "$@"; do
  case "$arg" in
    --release) profile=release ;;
    --serve) serve=true ;;
    *) echo "usage: scripts/web.sh [--release] [--serve]" >&2; exit 2 ;;
  esac
done
dir=$([ "$profile" = dev ] && echo debug || echo release)

export CARGO_TARGET_DIR=target/web
link_args=(--shared-memory --import-memory --max-memory=4294967296
  --export=__wasm_init_tls --export=__tls_size --export=__tls_align --export=__tls_base --export=__heap_base --export=__data_end)
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals ${link_args[*]/#/-C link-arg=}" \
  cargo +nightly build -p web --target wasm32-unknown-unknown --profile "$profile" \
  -Z build-std=std,panic_abort

out=dist/web
rm -rf "$out" && mkdir -p "$out"
wasm-bindgen "target/web/wasm32-unknown-unknown/$dir/web.wasm" --out-dir "$out" --target web
cp crates/web/static/index.html "$out/"

if $serve; then
  port=${PORT:-8080}
  echo "serving $out on http://localhost:$port"
  python3 -I - "$out" "$port" <<'PY'
import functools, http.server, sys
class Isolated(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()
handler = functools.partial(Isolated, directory=sys.argv[1])
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[2])), handler).serve_forever()
PY
fi
