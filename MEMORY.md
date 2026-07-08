# Memory — ai-meeting-agent

Append-only session log. Add a dated `### yyyy/mm/dd` entry per session; never edit past ones.

### 2026/07/08
**Duration**:
- 2026/07/08_10:35 - 10:40 (0.1h): Explained why `graphify-out-vexa/` and `.claude/` showed as untracked; fixed `.gitignore` to cover renamed graph-output dirs.

**Summary**: `graphify-out-vexa/` is the knowledge graph of the Vexa upstream snapshot (repo's `mirror` branch, commit `9d6647e`) that backs PRD.md Appendix D's pattern analysis — a prior session named it `-vexa` deliberately to avoid colliding with a future graph of this repo itself, but the literal `graphify-out/` entry in `.gitignore` didn't match the renamed folder. Changed the pattern to `graphify-out-*/` so any tagged graphify output dir is ignored. `.claude/` was untracked (not ignored) — only `.claude/settings.local.json` and `.claude/model-policy.state.json` are excluded by design, the rest of `.claude/` (currently `settings.json`) is meant to be committed and had simply never been added yet.
