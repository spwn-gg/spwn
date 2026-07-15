# Minimal dev image. Only DEPENDENCY inputs (this file + package*.json) determine
# the content tag spwn reuses across a fork lineage — source is bind-mounted, so a
# code edit never rebuilds the image.
FROM node:22-alpine
WORKDIR /app
COPY package.json package-lock.json* ./
RUN npm install
COPY . .
CMD ["npm", "run", "dev"]
