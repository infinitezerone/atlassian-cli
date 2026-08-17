# Error Codes & Exit Codes

Load this file when a command returns an error and the agent needs to react programmatically (retry, adjust parameters, or surface the issue).

Every error is emitted as JSON on stderr with `status` / `code` / `message` / `suggestion` (plus optional `detail` / `module` / `suggested_command`). Use `code` and exit codes to react programmatically.

| code | exit | meaning | suggestion |
| :--- | :--- | :--- | :--- |
| `AUTH_EXPIRED` | 10 | HTTP 401, PAT invalid/expired | Update the PAT: `atlassian-cli config set <module> --stdin` or use env vars |
| `PERMISSION_DENIED` | 11 | HTTP 403 | Check token permissions or contact admin |
| `NOT_FOUND` | 20 | HTTP 404 / resource or transition not found | Verify Key/ID/URL and Base URL prefix, or search first |
| `PARAM_INVALID` | 2 | Bad parameters / missing `--confirm` | Check `atlassian-cli <command> --help`; write ops also need `--confirm` |
| `CONFIG_MISSING` | 3 | URL/Token not configured | Run `atlassian-cli login` or set env vars |
| `HTTP_ERROR` | 1 | Other HTTP/network/parse errors | Check network, retry; `-k` only when the user approved it |
| `UNKNOWN_ERROR` | 1 | Fallback | Inspect `message`/`detail` |

Example:
```json
{"status":"error","code":"AUTH_EXPIRED","message":"认证失败: PAT Token 无效或已过期","module":"jira","suggestion":"重新生成/更新 PAT Token 后重试: atlassian-cli config set jira --stdin (...)"}
```

## Recovering from a failed write op

- A `PARAM_INVALID` error often means `--confirm` was omitted. The `suggested_command` field (when present) contains the exact command re-run with `--confirm` appended.
- `AUTH_EXPIRED` (exit 10): credentials stale — do not retry blindly; surface to the user.
- `NOT_FOUND` (exit 20): wrong key/ID/URL — verify with a read command (`jira get` / `confluence get` / `bitbucket get-pr`) before retrying.
