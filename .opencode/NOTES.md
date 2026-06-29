# Meeting Agent — Notes

**Architecture Context:** See `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/study-notes/14_Simple_Whisper_Transcription_Service.md`

---

## Key Decisions

### Phase 3 Decision Points (Pending)
1. **DB Schema Strategy:** TBD
   - Keep `transcript_settings` table, add `customStt` provider?
   - Create new minimal table?
   - Keep table structure, remove code paths only?

2. **Provider Scope:** TBD
   - Single `CustomSttConfig` (generic OpenAI-compatible)?
   - Multiple named providers (OpenAI, Groq, Deepgram, Custom)?

3. **Legacy Field Handling:** TBD
   - Set `whisper_model` to null?
   - Reuse as cloud model field?
   - Keep separate but unused?

---

## Implementation Context

**Two-Repo Structure:**
- **Extern:** `/Users/kagchi/Documents/projects/bmw-ntust-internship` (branch `2026-TEEP-5-Samuel`)
- **Workspace:** This repo (branch `main`)

**Plan Files:** `.opencode/plans/phase-*.md` (deleted when phase complete)

**Sync Rule:** Changes here → extern todo → issue #812 comment (in that order)

---

## Session Log

### 2026-06-26 14:54 CST
- Initialized session
- Clarified two-repo structure
- Planned `.opencode/` file structure (ACTIVE.md, TODO.md, NOTES.md)
- Established bidirectional sync strategy for TODO.md
- Created local workspace files
