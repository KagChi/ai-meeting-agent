# TODO — ai-meeting-agent

Build track for the hybrid architecture (see [PRD.md](PRD.md), [deploy/README.md](deploy/README.md)).
Branch: `ian`.

## Now
- [x] PRD pivot to hybrid (Vexa spine + lab-intelligence layer)
- [x] Deployment blueprint: `deploy/` compose + Dockerfile.server + .env + runbook
- [x] Deploy: Vexa **record-only** overlay (`deploy/docker-compose.bots.yml` include;
      `TRANSCRIBE_ENABLED=false`; ASR stays on meeting-agent import path)
- [ ] **Phase 0 — bring-up spike** (needs DGX + Docker; not doable on this laptop):
  - [ ] Clone Vexa; `docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.bots.yml up`
  - [ ] Mint `VEXA_API_KEY`; `POST :18056/bots` to a Teams/Meet test call
  - [ ] Confirm **recording** in Vexa MinIO (no Vexa live transcript required)
  - [ ] `meeting-agent import <recording>` → transcript via our WhisperX

## Next
- [ ] **Phase 1 — canonical pipeline (DGX):** WhisperX large-v3 re-transcription pass;
      per-segment language ID for EN/ZH/ID code-switch; feed diarization.
- [ ] **Phase 2 — speaker identification (Rust, extend `diarize` crate):**
  - [ ] Standalone embedding extraction from an audio turn (sherpa-onnx 3D-Speaker)
  - [ ] Voiceprint store: `/v1/voiceprints` enroll / list / delete (persist to `VOICEPRINT_DIR`)
  - [ ] `/v1/identify`: cosine-match diarized turns → person or `Guest-N` (`IDENTIFY_THRESHOLD`)
  - [ ] Rebuild `diarize-server` image; verify `cargo fmt/clippy/test` on a Rust host
- [ ] **Phase 3 — SOP minutes + actions:** generator emitting the exact
      `bmw-ece-ntust/SOP` `logistics/meeting.md` template; per-attendee action-item
      checkboxes; human `Reviewed by` certification gate.

## Later
- [x] **Phase 4 v1 — orchestrator in core + server:** meeting-end → download recording →
      `run_import_memory` (POST `/webhooks/vexa`, POST `/orchestrator/import`, runs table)
- [x] **Meeting-bot service (v1 Teams):** `services/meeting-bot` (Bun/Elysia/SQLite/local WAV)
      + agent proxy `POST/GET/DELETE /bots` (Meetily → agent only)
- [ ] **Phase 4 remainder:** auto-summary / SOP minutes → review gate → daily-log + GCal
- [ ] Meetily UI: join meeting via agent `/bots` + job poll
- [ ] **Phase 5 — mobile (optional):** one-tap in-person recorder → same ingest API.
- [ ] **Phase 6 — realtime (optional):** live captions / MCP agent hooks off Vexa WS.
- [ ] Consent + retention policy before enabling identification on real meetings (BIPA risk, PRD §8).

## Housekeeping (needs user decision — see chat)
- [ ] Decide fate of `.opencode/` (Samuel's internship workflow) on the `ian` branch
- [ ] `tests/output/*.json` are test-generated artifacts — gitignore vs keep tracked?
- [ ] Re-run `/graphify --update` on the Vexa `mirror` clone to finish semantic doc
      extraction (63/147 docs done; PRD.md Appendix D structural conclusions are
      unaffected, but the 1,816 dangling-endpoint edges mostly trace to the unprocessed docs)
