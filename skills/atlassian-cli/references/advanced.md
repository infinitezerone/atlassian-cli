# Advanced Topics: Introspection, Idempotency & Audit

Load this file when the task involves runtime command discovery, retry-safety semantics, or tracing what was changed. Not needed for routine read/write operations.

## Command Introspection (Schema)

Discover available commands at runtime instead of relying on docs:

```bash
atlassian-cli schema                  # full command tree (JSON)
atlassian-cli schema jira             # subtree for jira
atlassian-cli schema jira comment     # single command with args
```

Returns `name` / `about` / `args` (long, short, required, default, global) / `subcommands` for each node. Unknown paths return `NOT_FOUND` (exit 20).

## Idempotent Writes (AI Retry Safety)

Identical write requests (same method + path + body) are automatically deduplicated within a window (default **300 seconds**). If the same write was already executed, the CLI **skips the request** and returns:

```json
{"status":"idempotent_replay","action":"skipped","method":"POST","path":"/rest/api/2/issue/PROJ-123/comment","matched_at":1786809936,"hint":"窗口期内已执行过相同写操作,已跳过..."}
```

- **Exit code 0 — treat as success, do NOT retry.**
- If a retry is genuinely required: `ATLASSIAN_CLI_FORCE_WRITE=1` bypasses the dedupe.
- Adjust the window: `ATLASSIAN_CLI_IDEMPOTENCY_WINDOW=<seconds>` (0 disables).
- Multipart uploads (`attach`) are excluded from dedupe.

## Audit Trail (What Was Changed)

Every successful write op (and every idempotent replay) is appended to `~/.atlassian-cli/audit.jsonl`: timestamp, method, path, status, `replayed` flag and a 200-char body preview. **Tokens never appear** (PAT lives in HTTP headers, never in bodies).

```bash
atlassian-cli audit               # last 20 entries, newest first
atlassian-cli audit --limit 50
```

Use it to verify what the AI actually changed and when. Entries marked `"replayed": true` were deduplicated writes that did NOT hit the server.

**Disk protection**: `audit.jsonl` auto-rotates to `audit.1.jsonl` (keeps one backup) past 5MB (`ATLASSIAN_CLI_AUDIT_MAX_BYTES` to tune). There is **no separate idempotency file** — the dedupe fingerprint lives in each audit entry, so every write op performs exactly **one** disk append. Replay lookups scan only the recent tail of the file (records are time-ordered; scanning stops at the window boundary). No unbounded growth, no extra write amplification.
