# Use the official Puppeteer image, pinning to a specific version
FROM ghcr.io/puppeteer/puppeteer:24.11.2

# The official image runs as the non-root 'pptruser'.
# We will set the working directory inside the user's home.
WORKDIR /home/pptruser/app

# Install dependencies for sharp
USER root
RUN apt-get update && apt-get install -y libvips-dev
USER pptruser

# Copy the package.json and install dependencies.
# npm will use the browser provided by the image instead of downloading a new one.
COPY package.json .
RUN npm install

# Copy the rest of the application code (including TypeScript file)
COPY . .

# Expose the port the app runs on
EXPOSE 5812

# Run the script. This overrides the default CMD of the base image.
CMD [ "npm", "start" ]
