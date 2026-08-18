---
name: atlassian-cli
description: Operate self-hosted Atlassian (Jira / Confluence / Bitbucket Server & Data Center) via the atlassian-cli JSON CLI. Trigger whenever the user mentions Jira issue keys or links (PROJ-123), needs to read/search/comment/transition issues, log worklog, clone or bulk-create issues, look up users or JQL field suggestions, search or edit Confluence pages, or review Bitbucket PRs (diff, comments, approve).
---

# Atlassian CLI (`atlassian-cli`) Quick Reference

Self-hosted (Server / Data Center) Jira, Confluence, and Bitbucket CLI. All outputs are single-line compact JSON by default (use `--pretty` only for formatted human viewing).

## 1. Golden Principles

1. **Introspect Schema First**: Never guess flags or types. Run `atlassian-cli schema <command>` (e.g. `atlassian-cli schema jira comment`) to inspect exact parameter signatures.
2. **Resolve Entities Before Writing**:
   - People / Mentions: `jira user "Name"` ➔ `[~username]`
   - Issue Types: `jira issue-types --project PROJ`
   - Transitions: `jira transitions PROJ-123`
   - Custom Fields: `jira fields -q "Sprint"`
   - PR Diffs: `bitbucket diff-pr 100 --stat` (check file list before full diff)
3. **Write Safety**: All write operations require `--confirm` (exit 2 otherwise). Use `--dry-run` to preview mutation payloads safely.

## 2. Command Cheat-Sheet

**Jira (Read):**
```bash
atlassian-cli jira get PROJ-123                       # issue + comments (accepts browse URL)
atlassian-cli jira search "project = PROJ AND status != Closed" --limit 10
atlassian-cli jira projects                           # list projects (--query filter)
atlassian-cli jira issue-types --project PROJ         # available issue types
atlassian-cli jira user "John"                        # resolve [~username] for mentions
atlassian-cli jira suggest-values --field assignee --query "John"     # JQL candidate values
atlassian-cli jira transitions PROJ-123               # valid next status transitions
atlassian-cli jira worklog-list PROJ-123              # history worklogs
atlassian-cli jira watchers PROJ-123                  # watching users
```

**Jira (Write — always `--confirm`):**
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
atlassian-cli confluence get 12345678                 # plain text (--title-only to skip body)
atlassian-cli confluence create --space PROJ --title "..." --body "..." --confirm
atlassian-cli confluence update 12345678 --find "old" --replace "new" --confirm
```

**Bitbucket:**
```bash
atlassian-cli bitbucket list-prs --project PROJ --repo my-repo --state OPEN
atlassian-cli bitbucket get-pr 100                    # PR overview
atlassian-cli bitbucket diff-pr 100 --stat            # changed files summary (token saver!)
atlassian-cli bitbucket diff-pr 100 --file "App.java" # targeted single file diff
atlassian-cli bitbucket comment-pr 100 --body "LGTM" --confirm
atlassian-cli bitbucket approve-pr 100 --confirm
```

## 3. On-Demand Deep References

| Scope | Reference Path |
| :--- | :--- |
| JQL, mentions, attachments, custom fields, bulk/clone | `references/jira-commands.md` |
| CQL, page fetching, HTML macros, atomic editing | `references/confluence-commands.md` |
| PR diff line budgets, inline file review comments | `references/bitbucket-commands.md` |
| Exit codes & structured error recovery | `references/error-codes.md` |
| Schema tree, idempotency deduplication & audit trail | `references/advanced.md` |
