# Meeting bot — Bun + Playwright + Chromium (internal worker)
FROM oven/bun:1.2-debian

USER root
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    ffmpeg \
    pulseaudio \
    xvfb \
    libnss3 \
    libnspr4 \
    libatk1.0-0 \
    libatk-bridge2.0-0 \
    libcups2 \
    libdrm2 \
    libdbus-1-3 \
    libxkbcommon0 \
    libatspi2.0-0 \
    libxcomposite1 \
    libxdamage1 \
    libxfixes3 \
    libxrandr2 \
    libgbm1 \
    libasound2 \
    libpango-1.0-0 \
    libcairo2 \
    fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY package.json bun.lock* ./
RUN bun install --frozen-lockfile || bun install
RUN bunx playwright install chromium

COPY . .
ENV BOT_PORT=8091 \
    BOT_HOST=0.0.0.0 \
    DATA_DIR=/data \
    HEADLESS=true

VOLUME ["/data"]
EXPOSE 8091

# Xvfb when not truly headless-display; HEADLESS=true still uses Chromium headless
CMD ["bun", "run", "src/index.ts"]
