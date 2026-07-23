#!/usr/bin/env bash
# Start PulseAudio (null sink) so ffmpeg -f pulse can open "default".
# Browser audio may still need routing; this at least creates a valid capture device.
set -euo pipefail

export HOME="${HOME:-/root}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"
mkdir -p "$XDG_RUNTIME_DIR"

if command -v pulseaudio >/dev/null 2>&1; then
  pulseaudio --daemonize=true --exit-idle-time=-1 --log-target=stderr --disallow-exit 2>/dev/null || true
  # Ensure a sink exists for "default"
  if command -v pactl >/dev/null 2>&1; then
    pactl load-module module-null-sink sink_name=bot_sink 2>/dev/null || true
    pactl set-default-sink bot_sink 2>/dev/null || true
  fi
  sleep 0.5
fi

exec bun run src/index.ts
