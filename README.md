# atlassian-cli

> Unified AI-native CLI for Atlassian Self-Hosted (Data Center / Server) suite: **Jira + Confluence + Bitbucket**.

Designed for both human developers and **AI Coding Agents**. All commands output token-optimized, slim JSON payloads with built-in exponential backoff retries, connection timeout controls, and automated path encoding.

---

## 🚀 Quickstart

### 1. Installation

**Homebrew (macOS Recommended ⭐️)**
```bash
brew install infinitezerone/tap/atlassian-cli
```

**One-line Shell Script**
```bash
curl -fsSL https://raw.githubusercontent.com/infinitezerone/atlassian-cli/main/install.sh | sh
```

**Build from Source**
```bash
git clone https://github.com/infinitezerone/atlassian-cli.git
cd atlassian-cli && cargo build --release
```

### 2. Initial Setup

```bash
atlassian-cli login   # Interactive setup for Base URL & PATs with auto-connectivity check
atlassian-cli status  # Inspect connection status & authenticated user identity
```
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
| | `jira create --project P --summary S ...` | Create new issue (supports type/desc/labels/assignee/priority) |
| | `jira update <KEY/URL> [--summary S] ...` | Update existing issue fields (supports summary/desc/assignee/priority/labels) |
| | `jira assign <KEY/URL> <ASSIGNEE>` | Assign/reassign issue to a user (auto-sanitizes `[~...]` / `@{...}`) |
| | `jira assignable-users <KEY/URL> [Q]` | Search valid assignable users for an issue (matches webpage autocomplete) |
| **Confluence** | `confluence search <Q>` | Full-text search pages |
| | `confluence get <ID/URL> [--raw]` | Fetch page body (plain-text by default; supports `--max-chars` & `--offset`) |
| **Bitbucket** | `bitbucket list-prs` | List PRs by repo/state (OPEN/MERGED/DECLINED/ALL; accepts repo URL) |
| | `bitbucket get-pr <URL/ID>` | Fetch Pull Request overview |
| | `bitbucket user <QUERY>` | Search users by name/email (returns disambiguation info & `mention_syntax`) |
| | `bitbucket diff-pr <URL/ID>` | View PR code diff & changed file list |
| | `bitbucket comments-pr <URL/ID>` | Fetch PR comment tree & discussions |
| | `bitbucket comment-pr <URL/ID> --text "..."` | Post a code review comment |
| | `bitbucket create-pr ...` | Create a Pull Request |
| **Config** | `config set <module>` | Masked interactive Token configuration (`--stdin` for pipe input) |
| | `config set-url <module> <URL>` | Update Base URL for a module |
| | `config test` / `config status` | Verify connectivity & Token permissions |

*Note: For self-signed TLS certificates in enterprise environments, append `--insecure` (or `-k`), e.g., `atlassian-cli --insecure jira get PROJ-1`.*

---

## 🔒 Security Principles

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
