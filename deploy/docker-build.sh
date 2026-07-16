#!/usr/bin/env bash
# ============================================================================
# AI Meeting Agent — Docker build helper (x86_64)
# MCP is CLI-only (see .github/workflows/mcp-cli-artifacts.yml); no MCP image.
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

SERVER_IMAGE_NAME="${SERVER_IMAGE_NAME:-ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-agent-server}"
DIARIZE_IMAGE_NAME="${DIARIZE_IMAGE_NAME:-ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-agent-diarize-service}"
IMAGE_TAG="${IMAGE_TAG:-latest}"

echo "==> Building meeting-agent-server Docker image (x86_64)"
echo "    Image: ${SERVER_IMAGE_NAME}:${IMAGE_TAG}"
echo "    Context: ${PROJECT_ROOT}"
echo ""

docker build \
  --platform linux/amd64 \
  -f "${SCRIPT_DIR}/Dockerfile.server" \
  -t "${SERVER_IMAGE_NAME}:${IMAGE_TAG}" \
  "${PROJECT_ROOT}"

echo ""
echo "==> Building meeting-agent-diarize-service Docker image (x86_64, CUDA)"
echo "    Image: ${DIARIZE_IMAGE_NAME}:${IMAGE_TAG}"
echo ""

docker build \
  --platform linux/amd64 \
  -f "${SCRIPT_DIR}/Dockerfile.diarize" \
  -t "${DIARIZE_IMAGE_NAME}:${IMAGE_TAG}" \
  "${PROJECT_ROOT}"

echo ""
echo "==> Build complete:"
echo "    ${SERVER_IMAGE_NAME}:${IMAGE_TAG}"
echo "    ${DIARIZE_IMAGE_NAME}:${IMAGE_TAG}"
echo ""
echo "To run the stack:"
echo "  cd ${SCRIPT_DIR}"
echo "  docker compose up -d"
echo ""
echo "To enable diarization, set in deploy/.env:"
echo "  DIARIZE_ENABLED=true"
echo "  DIARIZE_SERVICE_URL=http://diarize-service:8001"
echo "  DIARIZE_EXECUTION_MODE=auto"
