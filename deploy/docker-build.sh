#!/usr/bin/env bash
# ============================================================================
# AI Meeting Agent — Docker build helper (x86_64)
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

IMAGE_NAME="${IMAGE_NAME:-ghcr.io/bmw-ece-ntust/ai-meeting-agent/meeting-agent-server}"
IMAGE_TAG="${IMAGE_TAG:-latest}"

echo "==> Building meeting-agent-server Docker image (x86_64)"
echo "    Image: ${IMAGE_NAME}:${IMAGE_TAG}"
echo "    Context: ${PROJECT_ROOT}"
echo ""

# Build for x86_64 (linux/amd64)
docker build \
  --platform linux/amd64 \
  -f "${SCRIPT_DIR}/Dockerfile.server" \
  -t "${IMAGE_NAME}:${IMAGE_TAG}" \
  "${PROJECT_ROOT}"

echo ""
echo "==> Build complete: ${IMAGE_NAME}:${IMAGE_TAG}"
echo ""
echo "To run the stack:"
echo "  cd ${SCRIPT_DIR}"
echo "  docker compose up -d"
echo ""
echo "To enable diarization, set in deploy/.env:"
echo "  DIARIZE_ENABLED=true"
echo "  DIARIZE_EXECUTION_MODE=cpu"
