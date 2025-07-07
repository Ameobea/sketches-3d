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

docker-build:
  docker build -t dream:latest .

build-and-deploy:
  #!/bin/bash

  just build
  just docker-build
  docker save dream:latest | bzip2 > /tmp/dream.tar.bz2
  scp /tmp/dream.tar.bz2 debian@ameo.dev:/tmp/dream.tar.bz2
  ssh debian@ameo.dev -t "cat /tmp/dream.tar.bz2 | bunzip2 | docker load && docker kill dream && docker container rm dream && docker run -d --name dream --restart always --net host -e PORT=5814 dream:latest && rm /tmp/dream.tar.bz2" && rm /tmp/dream.tar.bz2

# ---

build-basalt:
  cd src/viz/wasm && just build-basalt

build-csg-sandbox:
  cd src/viz/wasm && just build-csg-sandbox

build-geoscript:
  cd src/viz/wasm && just build-geoscript

build-geodesics:
  cd src/viz/wasm && just build-geodesics
