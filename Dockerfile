FROM node:24.4-slim AS deps
WORKDIR /app

# `stubs/` must land before install: package.json resolves sharp to `file:./stubs/sharp`.
COPY package.json yarn.lock ./
COPY stubs ./stubs
RUN yarn install --frozen-lockfile

FROM deps AS builder

COPY . /app

# adapter-node's `precompress` brotli/gzips ~90MB of output on libuv's threadpool, which
# defaults to 4 threads and dominates build wall-clock.
RUN UV_THREADPOOL_SIZE=$(nproc) yarn run build

FROM node:24.4-slim AS prod-deps

WORKDIR /app
ENV NODE_ENV=production

COPY package.json yarn.lock ./
COPY stubs ./stubs
RUN yarn install --frozen-lockfile --production=true --ignore-scripts && yarn cache clean

FROM node:24.4-slim

WORKDIR /app
# Runtime level loading reads JSON/geo assets directly from disk.
ENV NODE_ENV=production
ENV LEVELS_DIR=/app/src/levels
ENV ASSETS_DIR=/app/src/assets
COPY --from=builder /app/build build/
COPY --from=prod-deps /app/node_modules node_modules/
# Keep the source tree available for runtime generator modules imported from `src/...`.
COPY --from=builder /app/src src/
COPY package.json .

CMD ORIGIN="http://127.0.0.1:5814" BODY_SIZE_LIMIT="500M" node ./build/index.js
