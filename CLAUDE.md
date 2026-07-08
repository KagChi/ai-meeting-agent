# ai-meeting-agent — Claude Code Adapter

Read `AGENTS.md` first and treat it as your base instructions. Global rules in
`~/CLAUDE.md` stack on top. Stub seeded by sync-to-all-repos.sh; run `/project-init`
to populate, or edit by hand.

## Conventions

- **Branch model**: `main` = final project, `dev` = development, `mirror` = adopted
  upstream source snapshotted verbatim for adaptation (e.g. Vexa `90e5c72`, VERSION
  0.10.6.3.14, at `mirror`@`9d6647e`).
- **Graphify output dirs**: default graph lives at `graphify-out/` (this repo). A
  graph of an *external* codebase (e.g. the Vexa mirror, for pattern-mining) gets a
  tagged dir instead, e.g. `graphify-out-vexa/` — both patterns (`graphify-out/`,
  `graphify-out-*/`) are gitignored.
