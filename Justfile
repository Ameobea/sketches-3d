set dotenv-load := true

build-wasm:
  cd src/viz/wasm && just build

run:
  just build-wasm
  bun run dev

build:
  just build-wasm
  bun run build

sync:
  rsync -avz -e "ssh -p 1447" casey@67.185.21.196:/home/casey/dream/static/ ./static
  rsync -avz -e "ssh -p 1447" casey@67.185.21.196:/home/casey/dream/src/ammojs/ ./src/ammojs

preview:
  bun run preview --host 0.0.0.0 --port 4800

deploy:
  phost update 3d patch build
