set dotenv-load := true

build-wasm:
  cd src/viz/wasm && just build

run:
  just build-wasm
  bun run dev

build:
  just build-wasm
  bun run build

preview:
  bun run preview --host 0.0.0.0

deploy:
  phost update 3d patch build
