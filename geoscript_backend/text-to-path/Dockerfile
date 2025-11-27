# Dockerfile for text-to-path Bun server

# use the official Bun image
FROM oven/bun:1 AS base
WORKDIR /usr/src/app

# install dependencies into temp directory
# this will cache them and speed up future builds
FROM base AS install
RUN mkdir -p /temp/prod
COPY package.json bun.lock* /temp/prod/
RUN cd /temp/prod && bun install --production

# copy production dependencies and source code into final image
FROM base AS release
COPY --from=install /temp/prod/node_modules node_modules
COPY index.ts conf.ts package.json ./

# Set environment variables (can be overridden at runtime)
ENV NODE_ENV=production
ENV PORT=3000

# run the app
USER bun
ENTRYPOINT [ "bun", "run", "index.ts" ]

