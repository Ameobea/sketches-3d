set dotenv-load := true

build-wasm:
  cd src/viz/wasm && just build

opt-wasm:
  cd src/viz/wasm && just opt

copy-wasm:
  cd src/viz/wasm && just copy-files

run:
  just build-wasm
  just opt-wasm
  just copy-wasm
  bun run dev

build:
  just build-wasm
  just opt-wasm
  just copy-wasm
  bun run build

upwasm:
  just build-wasm
  just copy-wasm

gen-api-client:
  openapi-generator-cli generate -g typescript-fetch -i https://3d.ameo.design/api/swagger/v1/swagger.json -o src/api

sync:
  rsync -avz -e "ssh -p 1447" casey@67.185.21.196:/home/casey/dream/static/ ./static
  rsync -avz -e "ssh -p 1447" casey@67.185.21.196:/home/casey/dream/src/ammojs/ ./src/ammojs

preview:
  bun run preview --host 0.0.0.0 --port 4800

deploy:
  phost update 3d patch build

build-and-deploy:
  just build && just deploy

# ---

build-basalt:
  cd src/viz/wasm && just build-basalt
