FROM node:20.9-slim AS builder

ADD . /app

WORKDIR /app

RUN yarn
RUN yarn run build

FROM node:20.9-slim

WORKDIR /app
COPY --from=builder /app/build build/
COPY --from=builder /app/node_modules node_modules/
COPY package.json .

CMD ORIGIN="http://127.0.0.1:5814" BODY_SIZE_LIMIT="500M" node ./build/index.js
