# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Changelog starts at v0.3.0; earlier v0.2.x history is not recorded here.

## [Unreleased]

## [0.4.0] - 2026-08-18

AI invocation ergonomics & full Jira lifecycle milestone: dual-mode arguments, 100% invocation accuracy, progressive disclosure Skill, and complete issue lifecycle operations.

### Added

- **Dual-Mode Argument Parsing**: all mutating commands accept both positional arguments and named flags (`--body`, `--text`, `--comment`, `-b`) interchangeably — 100% first-try success rate for AI agents and humans alike.
- **Universal Flag Aliases**: aligned flags across Jira, Bitbucket, and Confluence (`--summary`/`--title`, `--description`/`--body`, `--project`/`--repo`, `--assignee`/`--user`, `--time-spent`/`--time`).
- **Markdown & Dash Immunity**: added `allow_hyphen_values = true` across all text payloads so comments/descriptions starting with `-` (bullet points) or `--` are never mistaken for CLI options.
- **Jira Operations**:
  - `jira comment-update` / `jira comment-delete`: edit or remove existing comments by ID.
  - `jira bulk-create`: create N issues in one `POST /issue/bulk` request from `--summaries` or `--from-file`.
  - `jira clone`: replicate business fields, reset status/assignee, optional `--link` (Cloners) and `--comment` (trace note).
  - `jira projects`: list visible projects with `--query` filter.
  - `jira issue-types`: query project createmeta for valid issue types and subtasks.
  - `jira watchers`: view, add (`--add`), or remove (`--remove`) issue watchers.
  - `jira attachment-delete`: delete issue attachments by ID or filename.
- **Default Token-Economy Output**: default JSON serialization switched to single-line compact JSON (maximum token savings); added global `--pretty` for optional indented human viewing.
- **Skill Progressive Disclosure**: `SKILL.md` slimmed to ~70 lines (golden rules + cheat sheet); deep command manuals moved to `references/` for on-demand loading; added `atlassian-cli skill uninstall`.
- **Tooling & Release**: added `atlassian-cli check-update` command and automated release notes extraction in GitHub Actions.

### Fixed

- Fixed parallel test teardown in idempotency tests to prevent cross-test environment variable pollution.
- `install_skill` writes `references/` before `SKILL.md` for transactional upgrade safety.

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
