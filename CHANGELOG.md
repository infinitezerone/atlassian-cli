# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Changelog starts at v0.3.0; earlier v0.2.x history is not recorded here.

## [Unreleased]

### Added

- `jira comment-update` / `jira comment-delete`: edit or remove existing comments (id from `jira get`).
- `jira bulk-create`: create N issues in one `POST /issue/bulk` request from `--summaries` or `--from-file`, sharing a project/type/priority/labels/custom template.
- `jira clone`: replicate business fields (summary/description/issuetype/priority/labels/components/fixVersions/duedate/environment + `--extra-fields`), reset status/assignee/comments, optional `--link` (Cloners) and `--comment` (trace note on source).
- `jira projects [--query]`: list visible projects.
- `jira issue-types [--project]`: deduplicated issue types with subtask flag (via `/issue/createmeta`) — stop guessing `--issue-type`.
- `jira watchers [--add U] [--remove U]`: view/add/remove issue watchers (`--add`/`--remove` mutually exclusive, `--confirm` gated).
- `jira attachment-delete`: delete an attachment by ID or filename.
- Default output switched to single-line compact JSON to maximize token savings for AI/script pipelines; added global `--pretty` for optional indented formatting.
- `skill uninstall`: remove the whole skill (SKILL.md + references/) from all agent directories.
- Skill progressive disclosure: SKILL.md slimmed to a cheat-sheet; detailed command references moved to `references/` (jira/confluence/bitbucket/error-codes/advanced), `install_skill` deploys the full tree, `skill status` reports `references_complete`.

### Changed

- `install_skill` writes `references/` before `SKILL.md` so an interrupted install never leaves a new SKILL.md referencing missing references (upgrade-safe for legacy single-file installs).
- Simplified `PARAM_INVALID` confirmation suggestion text; human confirmation is explicitly delegated to the agent platform layer.

## [0.3.0] - 2026-08-17

AI-first architecture release: write guards, idempotency, audit trail, context economy & custom fields.

### Added

- **Security**: sanitize server-controlled text (prompt-injection masking, control-char stripping, `"sanitized": true` marker); structured error codes (`AUTH_EXPIRED`/`PERMISSION_DENIED`/`NOT_FOUND`/`PARAM_INVALID`/`CONFIG_MISSING`/`HTTP_ERROR`) with granular exit codes (2/3/10/11/20/1) and `suggestion`/`suggested_command`.
- **Write safety**: global `--dry-run` / `--confirm` — all write operations refuse to execute without `--confirm` (exit 2); zero-side-effect previews; `ATLASSIAN_CLI_ALLOW_UNCONFIRMED=1` migration escape hatch.
- **Idempotent writes**: identical write requests deduplicated within a window (`idempotent_replay`, exit 0) — AI retries can't create duplicates; `ATLASSIAN_CLI_IDEMPOTENCY_WINDOW` / `ATLASSIAN_CLI_FORCE_WRITE`.
- **Audit trail**: every write op (and replay) appended to `audit.jsonl` with `replayed` flag; `atlassian-cli audit` command; disk-protected (rotation + prune, no unbounded growth).
- **Context economy**: `jira search --fields/--start-at`; clap errors as structured JSON; stdout kept machine-pure (human text to stderr); `config path` JSON output.
- **Self-introspection**: `atlassian-cli schema [path...]` renders the clap command tree as JSON with `write: true` markers.
- **JQL & input guards**: local JQL syntax validation; `time_spent` unit validation; `--started` range validation; `[~mention]` syntax validation; `jira suggest-fields` / `jira suggest-values` (autocompletedata API); `ensure_clean_id` for resource IDs.
- **Custom fields**: `--custom KEY=VAL` and `--custom-json` on create/update; `jira fields` introspection.
- `jira attachment-download` (PAT-authenticated, ID or filename); Confluence search pagination; Bitbucket `diff-pr --stat/--file/--max-lines/--offset`.
- `Cargo.lock` committed for reproducible builds.

### Fixed

- `install.sh` now verifies SHA256 checksums against `checksums.txt` before installing release assets (supply-chain tamper protection; graceful degradation for old releases).

## [0.2.4] - 2026-08-14

Initial published release (install.sh + Homebrew tap + embedded agent Skill). Historical details not recorded; see git history.
