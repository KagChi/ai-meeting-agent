#!/usr/bin/env bash
# ============================================================================
# AI Meeting Agent — Docker build helper
# Builds: meeting-agent-server, diarize-service, meeting-bot
# MCP is CLI-only; no MCP image.
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

SERVER_IMAGE_NAME="${SERVER_IMAGE_NAME:-ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-agent-server}"
DIARIZE_IMAGE_NAME="${DIARIZE_IMAGE_NAME:-ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-agent-diarize-service}"
MEETING_BOT_IMAGE_NAME="${MEETING_BOT_IMAGE_NAME:-ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-bot}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
# Override e.g. PLATFORM=linux/arm64 on DGX Spark
PLATFORM="${PLATFORM:-linux/amd64}"

echo "==> Building meeting-agent-server Docker image"
echo "    Image: ${SERVER_IMAGE_NAME}:${IMAGE_TAG}"
echo "    Platform: ${PLATFORM}"
echo "    Context: ${PROJECT_ROOT}"
echo ""

docker build \
  --platform "${PLATFORM}" \
  -f "${SCRIPT_DIR}/Dockerfile.server" \
  -t "${SERVER_IMAGE_NAME}:${IMAGE_TAG}" \
  "${PROJECT_ROOT}"

echo ""
echo "==> Building meeting-agent-diarize-service Docker image (CUDA)"
echo "    Image: ${DIARIZE_IMAGE_NAME}:${IMAGE_TAG}"
echo ""

docker build \
  --platform "${PLATFORM}" \
  -f "${SCRIPT_DIR}/Dockerfile.diarize" \
  -t "${DIARIZE_IMAGE_NAME}:${IMAGE_TAG}" \
  "${PROJECT_ROOT}"

echo ""
echo "==> Building meeting-bot Docker image (Bun + Playwright)"
echo "    Image: ${MEETING_BOT_IMAGE_NAME}:${IMAGE_TAG}"
echo "    Context: ${PROJECT_ROOT}/services/meeting-bot"
echo ""

docker build \
  --platform "${PLATFORM}" \
  -f "${PROJECT_ROOT}/services/meeting-bot/Dockerfile" \
  -t "${MEETING_BOT_IMAGE_NAME}:${IMAGE_TAG}" \
  "${PROJECT_ROOT}/services/meeting-bot"

echo ""
echo "==> Build complete:"
echo "    ${SERVER_IMAGE_NAME}:${IMAGE_TAG}"
echo "    ${DIARIZE_IMAGE_NAME}:${IMAGE_TAG}"
echo "    ${MEETING_BOT_IMAGE_NAME}:${IMAGE_TAG}"
echo ""
echo "To run the stack:"
echo "  cp deploy/.env.example deploy/.env   # set MEETING_BOT_ENABLED=true, etc."
echo "  docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d"
echo ""
echo "Meetily / clients call only meeting-agent-server :8080 (POST /bots)."
echo "meeting-bot is internal (MEETING_BOT_URL=http://meeting-bot:8091)."
