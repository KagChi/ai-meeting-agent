@CLAUDE.md

## Working Agreement (read first)

- **No push without explicit permission.** Never `git push` until the user
  says "push", "ship it", or equivalent. Default: commit locally, then wait.
  This applies to every remote (bmw-ece-ntust/ai-meeting-agent,
  extern bmw-ntust-internship/internship, anything else).
- **Permission is per-push, never carried.** A prior "yes" — in this session
  or a previous one — does not authorize the next push. Re-ask before every
  push; never assume consent carries over.
- **Origin is `bmw-ece-ntust/ai-meeting-agent` (upstream lab repo).** A push to
  `origin` is a public write to the lab's repo — ask before any push.
- **No public write without explicit permission.** Same rule for `gh issue
  comment` / PATCH and any other public artifact. Default: draft locally,
  show the user, wait for "post" / "send it".
- **Ask before documenting to AGENTS.md or the #812 thread.** If you want
  to record a new rule, format, or behavior, ask first ("should I
  document this?"). The user curates.

## Pre-Commit Verification (mandatory)

Before every commit, run the following checks in order:

1. **Format check**: `cargo fmt --all -- --check`
   - If formatting issues found, run `cargo fmt --all` to fix them
2. **Lint check**: `cargo clippy --all --all-targets -- -D warnings`
   - All clippy warnings must be resolved before committing
3. **Test check**: `cargo test --all`
   - All tests must pass before committing

**Commit workflow:**
1. Run all pre-commit checks (fmt, clippy, test)
2. Fix any issues found
3. Stage only the intended files (avoid `git add .`)
4. **STOP and ask for permission to commit** — never commit without explicit permission
5. Only after receiving permission: create commit with clear, conventional message

**Permission is per-action and NEVER assumed:**
- Each commit requires explicit permission — **NEVER commit automatically**
- Each push requires explicit permission
- Permission never carries over from previous actions
- "commit & push" in one user message = permission for both actions in sequence
- **If you commit without permission, immediately undo with `git reset --soft HEAD~1`**

## Internship Track

**Two-repo structure:**
- **Extern repo** (`/Users/kagchi/Documents/projects/bmw-ntust-internship`, branch `2026-TEEP-5-Samuel`): Source of truth for todo, study notes, daily logs
- **ai-meeting-agent workspace** (this repo, branch `main`): Implementation, plan files, code changes

**Local workspace files (bidirectional sync with extern):**
- Active work: `.opencode/ACTIVE.md`
- Todo: `.opencode/TODO.md` ↔ `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md`
- Notes: `.opencode/NOTES.md` ↔ `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/study-notes/14_ai-meeting-agent_Cloud_First_Transcription.md`

## Active Plans

Per-phase action files live in this repo at `.opencode/plans/`. **One file
per phase**; each describes the work for that phase only, and is deleted from
this directory when the phase is complete.

Do **not** maintain a list of plan files in this document. The directory
itself is the source of truth — at the start of every session, list
`.opencode/plans/` to discover what's currently active, then read each file.
Files present in the directory = phases not yet done; absence = phase
complete (also reflected by the checkboxes in the source todo).

## Daily-Logs Sync Rule (mandatory)

The source todo in the extern repo is the single source of truth for phase
status. Whenever you complete work on this branch, you **must** update the
extern todo at
`/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md`
to check off the matching items (`- [ ]` → `- [x]`).

Sync both directions, in this order:

1. **Code complete → todo**: when an item in a phase plan file is done, edit
   the extern todo to flip the corresponding `[ ]` to `[x]`. Do this before
   removing the line from the phase plan file.
2. **Phase complete → plan**: when a whole phase is fully checked off in the
   todo, delete that phase's plan file from `.opencode/plans/` so the
   directory only describes remaining phases.
3. **Manual edits to the todo**: any time the todo itself is edited (renamed,
   re-scoped, items added/removed), mirror the change in the affected phase
   file.

## Extern Repo Commit & Push Rule (mandatory)

Edits to the extern repo
(`/Users/kagchi/Documents/projects/bmw-ntust-internship`, branch
`2026-TEEP-5-Samuel`) — the source todo, study notes, daily logs — must not
be left dangling locally. After the Daily-Logs Sync Rule triggers an edit to
the extern repo, commit and push in the **same turn**, before posting the
#812 comment:

1. **Commit — REQUIRES explicit permission.** The Working Agreement's "No
   commit without explicit permission" applies to the extern repo too. **STOP
   and ask for permission before staging and committing.** Never commit
   automatically. Stage the changed extern files and commit with message
   `chore: update` (matching the existing extern-repo style) only after
   receiving explicit permission. Do not craft a long custom message; the
   #812 comment carries the narrative.
2. **Push — still requires explicit permission.** After committing, ask
   ("extern repo has commits — push to origin/2026-TEEP-5-Samuel?") and wait
   for the user before `git push`. Default: commit locally, ask, wait.
3. **#812 links reference the pushed HEAD.** When the Issues Sync Rule posts
   a comment, any link to extern-repo content (todo, study notes) must point
   at the **pushed** commit, not a local-only SHA. If the push was deferred,
   note that in the comment and PATCH the link once pushed.

Rationale: the #812 comment thread and its links are the lab mentors' view of
progress; a link to unpushed content is a dead link. Keeping the extern remote
current also prevents drift between the two workspace mirrors.

## Issues Sync Rule (mandatory)

The intern's daily log lives as a comment thread on
[bmw-ntust-internship/internship issue #812](https://github.com/bmw-ntust-internship/internship/issues/812).
Whenever you complete a meaningful chunk of work on this branch, you **must**
post a continuation comment to that thread describing what was done. The
comment is public, so do not post secrets, internal paths beyond the repo
root, or anything you would not want the lab mentors to read.

Comment format (matches the existing daily-log pattern in the thread):

```
# YYYY/MM/DD

## Short-term Goals
#### <one-line summary of this session's slice>

**Daily Logs:**
- HH:MM-HH:MM <what you did, with concrete file/function pointers>

**Links:**
- <relevant URLs — todo, study notes, plan files>
- Commits: <repo>/compare/<first_commit>...<last_commit>
```

**Comment content guidelines:**
- Use commit-based timestamps from `git log` for accuracy
- Include only code changes and technical work (no planning/discussion unless it resulted in code)
- Reference the BMW upstream repo (`bmw-ece-ntust/ai-meeting-agent`) in all workspace links, not personal forks
- Keep format minimal: date, goal, time ranges with brief descriptions, links
- Use commit range instead of single "latest commit" link to show all commits in the session
- Get first commit: `git log --oneline --reverse | head -1`
- Get last commit: `git log --oneline -1`

Sync in lockstep with the Daily-Logs Sync Rule:

1. **Code complete → comment**: when you flip items in the extern todo, update
   the #812 thread in the same turn. **One comment per day** — if a comment
   for today already exists, PATCH it in place to append the new work; only
   post a new comment (`gh issue comment 812 --repo
   bmw-ntust-internship/internship --body-file <tmp>`) when no comment exists
   yet for the current calendar day.
2. **Comment → plan link**: when you add a new plan file to `.opencode/plans/`
   that the user should be able to read from the thread, include its GitHub
   URL in the comment's Links block.
3. **Daily-log daily**: post at least one comment per working day even if no
   code changed — a one-liner is fine ("no code changes; planning only").
4. **Edit in place, never delete**: if a comment needs a fix (wrong time,
   typo, broken link), edit it in place with the `gh api` PATCH endpoint —
   `PATCH repos/bmw-ntust-internship/internship/issues/comments/<id>` — do
   **not** delete and repost. **Default to PATCH for any correction**, even
   trivial ones (time ranges, single typos, link fixes). A gap in the thread
   is a permanent public record of the mistake, and the thread is the lab
   mentors' log of progress. The only narrow exception is a comment posted
   within the last few minutes with no replies and nothing depending on its
   id; even then, prefer edit.
   
   **Single-PATCH correction workflow:** When you need to make multiple
   corrections to the same comment (iterative feedback from the user), PATCH
   the same comment multiple times in sequence. Do not create multiple new
   comments or ask whether to post a correction comment — just PATCH the
   existing one directly. Use the same comment ID for all iterations until
   the user is satisfied with the result.
5. **Ask before documenting to the issue**: before posting a new comment to
   #812 that documents a decision, rule, format, or piece of behavior,
   **ask the user first** ("should I document this on #812?"). The user
   curates the public thread and may want the change kept as a one-off
   instead. This applies to new comments, not to PATCHes fixing an
   already-posted comment (step 4 covers that case).

When a fresh session lands on this branch, the first action is: read
`AGENTS.md` (which proxies `CLAUDE.md` via the `@CLAUDE.md` token at the top
— read `CLAUDE.md` next so the project-level build/test/conventions are
in context), scan `.opencode/plans/`, read the source todo, scan the last 3
comments on issue #812 to see the most recent context, reconcile any drift
between them, then proceed.
