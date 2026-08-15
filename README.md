# atlassian-cli

> Unified AI-native CLI for Atlassian Self-Hosted (Data Center / Server) suite: **Jira + Confluence + Bitbucket**.

Designed for both human developers and **AI Coding Agents**. All commands output token-optimized, slim JSON payloads with built-in exponential backoff retries, connection timeout controls, and automated path encoding.

---

## 🚀 Quickstart

### 1. Installation

**Homebrew (macOS Recommended ⭐️)**
```bash
# 1. (新版 Homebrew 首次使用需信任该 Tap)
brew trust infinitezerone/tap

# 2. 安装 atlassian-cli
brew install infinitezerone/tap/atlassian-cli
```

**One-line Shell Script (CDN Accelerated)**
```bash
curl -fsSL https://cdn.jsdelivr.net/gh/infinitezerone/atlassian-cli@main/install.sh | sh
```

**Build from Source**
```bash
git clone https://github.com/infinitezerone/atlassian-cli.git
cd atlassian-cli && cargo build --release
```

### 2. Initial Setup

```bash
atlassian-cli login          # Interactive setup with auto-connectivity check (paste ANY webpage URL from browser!)
atlassian-cli skill install  # Auto-deploy embedded official AI Agent Skill to ~/.gemini/config/skills/
atlassian-cli status         # Inspect connection status & authenticated user identity
```
*💡 **Tip**: When configuring Base URL, you can just paste ANY webpage URL from your browser (e.g. `https://jira.company.com/browse/PROJ-123`). The CLI will automatically extract the clean Base URL and probe subpaths!*
*Environment variables are also supported: `JIRA_URL` / `JIRA_TOKEN`, `CONFLUENCE_URL` / `CONFLUENCE_TOKEN`, `BITBUCKET_URL` / `BITBUCKET_TOKEN`.*

---

## 🛠️ Command Reference

> 💡 **Pro-tip**: All Jira Issue Keys, Confluence Page IDs, and Bitbucket PR targets accept **direct browser webpage URLs**.

| Product | Command | Description |
| :--- | :--- | :--- |
| **Auth** | `login [MODULE]` / `setup` | Interactive setup with real-time probe & URL normalization (supports `login jira`) |
| | `status` / `whoami` | Inspect config state, TLS flags, and PAT identities |
| **Jira** | `jira search <JQL>` | JQL query search with slim JSON output |
| | `jira get <KEY/URL>` | Fetch issue details & comments (accepts Key or browser URL, `--comments-limit`) |
| | `jira user <QUERY>` | Search users by name/email (returns disambiguation info & `mention_syntax`) |
| | `jira comment <KEY> <TEXT>` | Add a comment to an issue |
| | `jira transition <KEY> <STATUS>` | Transition issue status (e.g. In Progress, Done) |
| | `jira transitions <KEY/URL>` | Inspect all available status transitions & target statuses for an issue |
| | `jira link <FROM> <TO> [--type "Relates"]` | Link two Jira issues together (supports Relates, Blocks, Cloners, Duplicate) |
| | `jira attachments <KEY/URL>` | List all attachments with filename, size, and download URLs |
| | `jira attach <KEY/URL> <FILE>` | Upload a local file to a Jira issue |
| | `jira create --project P --summary S ...` | Create new issue (supports type/desc/labels/assignee/priority) |
| | `jira update <KEY/URL> [--summary S] ...` | Update existing issue fields (supports summary/desc/assignee/priority/labels) |
| | `jira assign <KEY/URL> <ASSIGNEE>` | Assign/reassign issue to a user (auto-sanitizes `[~...]` / `@{...}`) |
| | `jira assignable-users <KEY/URL> [Q]` | Search valid assignable users for an issue (matches webpage autocomplete) |
| | `jira worklog-add <KEY/URL> <TIME> [--comment C]` | Log time spent on an issue (supports `"2h 30m"`, `"1d"`, `"45m"`, `--comment`, `--started`) |
| | `jira worklog-list <KEY/URL>` | List all logged worklog entries & time spent on an issue |
| | `jira worklog-delete <KEY/URL> <WORKLOG_ID>` | Delete a specific worklog entry from an issue |
| **Confluence** | `confluence search <Q> [-t] [-s S]` | Search pages (full-text by default; `-t` for title-only search, `-s` for space filter) |
| | `confluence get <ID/URL> [-t] [--raw]` | Fetch page body (plain-text by default; `-t` for title/meta only, `--max-chars` & `--offset`) |
| | `confluence children <ID/URL>` | List direct child pages with titles, IDs & versions (0-body lightweight inspection) |
| | `confluence spaces [--query Q]` | List or search all accessible Confluence spaces & Space Keys |
| | `confluence attachments <ID/URL>` | List all page attachments with filename, size, and download URLs |
| | `confluence attach <ID/URL> <FILE>` | Upload a local file to a Confluence page |
| | `confluence create --space S --title T --body B` | Create page (supports Date `<time>` pills, Jira issue cards & Mermaid diagrams) |
| | `confluence update <ID/URL> [--find F --replace R]` | Safe page update (supports 1-match `--find/--replace`, `--append`, `--prepend`, `--dry-run`) |
| **Bitbucket** | `bitbucket list-prs` | List PRs by repo/state (OPEN/MERGED/DECLINED/ALL; accepts repo URL) |
| | `bitbucket get-pr <URL/ID>` | Fetch Pull Request overview |
| | `bitbucket user <QUERY>` | Search users by name/email (returns disambiguation info & `mention_syntax`) |
| | `bitbucket diff-pr <URL/ID>` | View PR code diff & changed file list |
| | `bitbucket comments-pr <URL/ID>` | Fetch PR comment tree & discussions |
| | `bitbucket comment-pr <URL/ID> --text "..."` | Post a comment on PR (supports `--file <PATH>` & `--line <NUM>` for inline comments) |
| | `bitbucket create-pr --project P --repo R ...` | Create PR (auto-loads web default reviewers, supports extra `--reviewers`) |
| | `bitbucket approve-pr <URL/ID>` | Approve Pull Request |
| **Config** | `config set <module>` | Masked interactive Token configuration (`--stdin` for pipe input) |
| | `config set-url <module> <URL>` | Update Base URL for a module |
| | `config test` / `config status` | Verify connectivity & Token permissions |
| **Introspect** | `schema [path...]` | Machine-readable command tree JSON for AI agents (e.g. `schema jira comment`) |

**Global Flags** (usable anywhere):

| flag | meaning |
| :--- | :--- |
| `--dry-run` | Preview write operations as JSON (`status: dry_run`) without calling the API — zero side effects |
| `--confirm` | Explicitly confirm write operations; **without it the CLI refuses to execute** (exit 2) |
| `-k` / `--insecure` | Skip TLS certificate validation (MITM risk — only with user approval) |

*Note: For self-signed TLS certificates in enterprise environments, prefer configuring the CA as trusted. If `--insecure` (or `-k`) must be used, only do so after informing the user that certificate validation is disabled.*

---

## 🔒 Security Principles

- **Write-Operation Guard**: All 15 write operations require `--confirm`; `--dry-run` prints a preview first. `ATLASSIAN_CLI_ALLOW_UNCONFIRMED=1` is a migration-only escape hatch.
- **Structured Errors**: Errors are JSON with `code` + `suggestion` + granular exit codes (2 param, 3 config, 10 auth, 11 permission, 20 not-found, 1 http/generic) — machine-actionable for agents.
- **Prompt-Injection Defense**: Server-controlled text (descriptions, comments, page bodies, PR comments, diffs) is sanitized before output; modified responses carry `"sanitized": true`.
- **POSIX Permissions**: Config file stored in `~/.atlassian-cli/config.json` with strict POSIX permissions (`0700` directory, `0600` file, readable only by owner).
- **Zero-History Leakage**: Supports stdin pipe (`echo "PAT" | atlassian-cli config set <module> --stdin`) to prevent sensitive tokens from appearing in shell history or process lists.

---

## 🏗️ Architecture

Built on a self-contained module contract (`AtlassianModule` trait) for zero dependency contamination:

```
src/
├── main.rs       # Minimal CLI dispatch entrypoint
├── module.rs     # AtlassianModule Trait contract
├── http.rs       # Unified HTTP Client (Retries / Timeouts / TLS)
├── config.rs     # Configuration persistence & authentication test
├── utils.rs      # URL -> Key/ID/PR parser utilities
├── jira.rs       # Jira module implementation
├── confluence.rs # Confluence module implementation
└── bitbucket.rs  # Bitbucket module implementation
```

## License

[MIT](LICENSE)
