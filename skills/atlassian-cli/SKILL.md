---
name: atlassian-cli
description: Operate self-hosted Atlassian (Jira / Confluence / Bitbucket Server & Data Center) via the atlassian-cli JSON CLI. Trigger whenever the user mentions Jira issue keys or links (PROJ-123), needs to read/search/comment/transition issues, log worklog, clone or bulk-create issues, look up users or JQL field suggestions, search or edit Confluence pages, or review Bitbucket PRs (diff, comments, approve).
---

# Atlassian CLI (`atlassian-cli`) Skill Guide

Use `atlassian-cli` to interact with self-hosted (Data Center / Server) Jira, Confluence, and Bitbucket instances. All output is JSON — machine-parseable, no guessing.

**Token-optimal by default**: All commands emit single-line compact JSON by default (zero whitespace token waste). Append `--pretty` if indented formatting is needed for human presentation.

## 1. Golden Rules for AI Agents

1. **Schema Self-Introspection (When Uncertain, Introspect First)**:
   Never guess parameters or argument formats. Run `schema` to discover the exact argument tree, types, and flags:
   ```bash
   atlassian-cli schema jira comment            # inspect specific command schema & flags
   atlassian-cli schema bitbucket comment-pr    # inspect args & write: true markers
   ```
2. **Entity Pre-Introspection (Read Before Write)**:
   Never guess entity names/IDs — always resolve real values first:
   - **Users & Mentions**: `atlassian-cli jira user "Name"` ➔ returns `mention_syntax` (`[~username]`)
   - **Issue Types**: `atlassian-cli jira issue-types --project PROJ` ➔ avoid guessing Bug vs Task
   - **Workflow Statuses**: `atlassian-cli jira transitions PROJ-123` ➔ avoid invalid transition names
   - **Custom Fields**: `atlassian-cli jira fields -q "Sprint"` ➔ find real `customfield_xxxxx` IDs
   - **JQL Candidates**: `atlassian-cli jira suggest-values --field assignee --query "..."`
   - **PR Diff Overview**: `atlassian-cli bitbucket diff-pr 100 --stat` ➔ check changed files before full diff
3. **Write-Operation Safety**:
   All write operations require `--confirm`; without it the CLI rejects execution (exit 2, `PARAM_INVALID`). Use `--dry-run` to preview the mutation payload with zero side effects:
   ```bash
   atlassian-cli --dry-run jira comment PROJ-123 --body "Analysis completed."   # preview
   atlassian-cli --confirm jira comment PROJ-123 --body "Analysis completed."   # execute
   ```

## 2. Core Command Cheat-Sheet

**Jira (read):**
```bash
atlassian-cli jira get PROJ-123                       # issue + comments (accepts full browse URL)
atlassian-cli jira search "project = PROJ AND status != Closed" --limit 10
atlassian-cli jira projects                           # list projects (--query filter)
atlassian-cli jira issue-types --project PROJ         # available issue types
atlassian-cli jira user "John"                      # resolve mention_syntax [~username]
atlassian-cli jira suggest-values --field assignee --query "John"   # JQL candidates
atlassian-cli jira transitions PROJ-123               # available status moves
atlassian-cli jira worklog-list PROJ-123
atlassian-cli jira watchers PROJ-123                  # who's watching
```

**Jira (write — always --confirm):**
```bash
atlassian-cli jira comment PROJ-123 --body "text" --confirm
atlassian-cli jira comment-update PROJ-123 <comment_id> --body "new text" --confirm
atlassian-cli jira comment-delete PROJ-123 <comment_id> --confirm
atlassian-cli jira transition PROJ-123 "Done" --confirm
atlassian-cli jira create --project PROJ --summary "..." --issue-type Task --confirm
atlassian-cli jira update PROJ-123 --summary "..." --confirm
atlassian-cli jira assign PROJ-123 --user john.doe --confirm
atlassian-cli jira worklog-add PROJ-123 --time-spent "2h 30m" --body "..." --confirm
atlassian-cli jira link PROJ-123 PROJ-456 --type "Relates" --confirm
atlassian-cli jira bulk-create --project PROJ --summaries "a,b,c" --confirm
atlassian-cli jira clone PROJ-123 --link --confirm
```

**Confluence:**
```bash
atlassian-cli confluence search "Architecture" --limit 5
atlassian-cli confluence get 12345678                 # page text (--title-only to skip body)
atlassian-cli confluence create --space PROJ --title "..." --body "..." --confirm
atlassian-cli confluence update 12345678 --find "old" --replace "new" --confirm
```

**Bitbucket:**
```bash
atlassian-cli bitbucket list-prs --project PROJ --repo my-repo --state OPEN
atlassian-cli bitbucket get-pr 100
atlassian-cli bitbucket diff-pr 100 --stat            # changed files only (token saver)
atlassian-cli bitbucket comment-pr 100 --body "LGTM" --confirm
atlassian-cli bitbucket approve-pr 100 --confirm
```

## 3. Reference Documents (load on demand)

Detailed command examples live in separate reference files — load only what the task needs:

| When you need... | Load |
| :--- | :--- |
| Full Jira commands (JQL details, mentions, attachments, fields, worklog, bulk/clone flags) | `references/jira-commands.md` |
| Full Confluence commands (CQL, page body fetch, macro create/update) | `references/confluence-commands.md` |
| Full Bitbucket commands (PR diff filters, inline comments) | `references/bitbucket-commands.md` |
| Handle an error / react to exit codes programmatically | `references/error-codes.md` |
| Runtime schema discovery, idempotent-write semantics, audit trail | `references/advanced.md` |
