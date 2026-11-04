# Mirrors ghcr.io/puppeteer/puppeteer's recipe on an unpinned Node base; that image pins a Node
# digest and lags Node security releases.
FROM node:24-bookworm-slim

ENV LANG=en_US.UTF-8 DBUS_SESSION_BUS_ADDRESS=autolaunch: PUPPETEER_CACHE_DIR=/home/pptruser/.cache/puppeteer
RUN groupadd -r pptruser && useradd -u 10042 -rm -g pptruser -G audio,video pptruser

WORKDIR /home/pptruser/app
COPY --chown=pptruser:pptruser package.json package-lock.json ./
RUN apt-get update \
  && apt-get install -y --no-install-recommends fonts-freefont-ttf dbus dbus-x11 unzip \
  && npm ci --omit=dev \
  && npx puppeteer browsers install chrome --install-deps \
  && chown -R pptruser:pptruser /home/pptruser \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

USER pptruser
COPY --chown=pptruser:pptruser . .

EXPOSE 5812
CMD [ "npm", "start" ]
