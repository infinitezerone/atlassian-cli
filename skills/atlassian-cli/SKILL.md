---
name: atlassian-cli
description: Manage Jira issues, search Confluence pages, and perform Bitbucket PR code reviews using atlassian-cli. Trigger whenever the user mentions Jira issue keys (e.g. PROJ-123), Jira ticket links, Confluence documentation searches, or Bitbucket PRs, diffs, and code review inline comments.
---

# Atlassian CLI (`atlassian-cli`) Skill Guide

Use `atlassian-cli` to interact with self-hosted (Data Center / Server) Jira, Confluence, and Bitbucket instances directly from the command line with zero-friction JSON outputs.

---

## 1. Setup & Environment Verification

Before executing Atlassian operations, verify connectivity and credentials if needed:

```bash
atlassian-cli status
```

- If credentials are missing or unconfigured, prompt the user to run `atlassian-cli login`.
- If using enterprise self-signed TLS certificates, append `--insecure` (or `-k`), e.g., `atlassian-cli --insecure jira get PROJ-123`.

---

## 2. Jira Workflow Commands

`atlassian-cli` accepts both **Issue Keys** (e.g., `PROJ-123`) and **direct browser webpage URLs** (e.g., `https://jira.example.com/browse/PROJ-123`).

### Read Issue & Discussion Comments
```bash
atlassian-cli jira get PROJ-123
# Control returned comments count (default 10, set 0 to hide comments)
atlassian-cli jira get PROJ-123 --comments-limit 5
```

### Search Issues (JQL)
```bash
atlassian-cli jira search "assignee = currentUser() AND status != Closed" --limit 10
```

### Add Comment to Issue
```bash
atlassian-cli jira comment PROJ-123 "Analysis completed. Pending code review."
```

### Transition Issue Status
```bash
atlassian-cli jira transition PROJ-123 "In Progress"
atlassian-cli jira transition PROJ-123 "Done"
```

### Create & Update Issues
```bash
# Create Issue
atlassian-cli jira create --project PROJ --summary "Fix login timeout bug" --issue-type Bug --assignee john.doe --priority High

# Update Issue fields
atlassian-cli jira update PROJ-123 --summary "Updated title" --priority Medium --labels "backend,urgent"
```

### Assign Issue & User Search
```bash
# Search assignable users for an issue (matches webpage autocomplete)
atlassian-cli jira assignable-users PROJ-123 "John"

# Look up username & mention_syntax ([~username])
atlassian-cli jira user "John"

# Assign issue (auto-sanitizes [~username] or @{username} input)
atlassian-cli jira assign PROJ-123 john.doe
```

---

## 3. Confluence Workflow Commands

### Search Documentation (CQL)
```bash
atlassian-cli confluence search "Architecture Design" --limit 5
```

### Fetch Page Body
```bash
# Fetch page text (default 8000 chars, accepts Page ID or browser URL)
atlassian-cli confluence get 12345678

# Paginate long documents using --offset
atlassian-cli confluence get 12345678 --offset 8000 --max-chars 8000

# Fetch raw HTML storage format
atlassian-cli confluence get 12345678 --raw
```

### Create & Update Pages (Macro-enabled)
```bash
# Create page (supports Date <time> pills & Jira issue cards)
atlassian-cli confluence create --space PROJ --title "Release Notes 6.2.0" \
  --body "Release date: <time datetime=\"2026-08-13\"/>\nRelated ticket: <ac:structured-macro ac:name=\"jira\"><ac:parameter ac:name=\"key\">PROJ-123</ac:parameter></ac:structured-macro>"

# Local search & replace update (token-efficient, unique 1-match safety)
atlassian-cli confluence update 12345678 --find "Target Version: v1.0" --replace "Target Version: v2.0"

# Preview update diff without committing (--dry-run)
atlassian-cli confluence update 12345678 --find "v1.0" --replace "v2.0" --dry-run

# Append new section to page tail
atlassian-cli confluence update 12345678 --append "## Discussion Notes\n- Approved by team"
```

---

## 4. Bitbucket Workflow Commands (Code Review)

### List & Create Pull Requests
```bash
# List OPEN PRs in a repository (supports repo webpage URL)
atlassian-cli bitbucket list-prs --url "https://gitpub.example.com/projects/PROJ/repos/my-repo" --state OPEN

# Create Pull Request (auto-loads web default reviewers, supports extra --reviewers)
atlassian-cli bitbucket create-pr --project PROJ --repo my-repo --title "Fix login timeout" --from feature/login-fix --to main --reviewers "john.doe, jane.smith"
```

### Inspect PR Details & Code Diffs
```bash
# Get PR overview (accepts PR ID or direct webpage URL)
atlassian-cli bitbucket get-pr https://gitpub.example.com/projects/PROJ/repos/my-repo/pull-requests/100

# View PR code diff & changed file list
atlassian-cli bitbucket diff-pr https://gitpub.example.com/projects/PROJ/repos/my-repo/pull-requests/100

# View PR comment tree & discussions
atlassian-cli bitbucket comments-pr https://gitpub.example.com/projects/PROJ/repos/my-repo/pull-requests/100
```

### Post Code Review Comments & Approve PR
```bash
# General PR comment
atlassian-cli bitbucket comment-pr 100 --text "LGTM, overall architecture is clean."

# Precise file line inline comment (Code Review)
atlassian-cli bitbucket comment-pr 100 \
  --text "Consider adding null-check here to prevent NullPointerException" \
  --file "src/main/java/App.java" \
  --line 42

# Approve Pull Request
atlassian-cli bitbucket approve-pr 100
```

---

## 5. Bitrise Mobile CI/CD Commands (Read-Only)

### List Apps & Accessible Repositories
```bash
atlassian-cli bitrise list-apps
```

### Search Build History & Status
```bash
# List builds for default or explicit app (supports --branch, --workflow, --status, --limit)
atlassian-cli bitrise list-builds --app my-repo --branch main --limit 5

# Filter failed builds (0: running, 1: success, 2: failed, 3: aborted)
atlassian-cli bitrise list-builds --status 2
```

### Inspect Build Details & Read Logs
```bash
# Get build details (accepts build slug or direct webpage URL)
atlassian-cli bitrise get-build https://app.bitrise.io/build/7c8e9f2a0b1c

# Read build console execution logs (supports --max-chars)
atlassian-cli bitrise get-log https://app.bitrise.io/build/7c8e9f2a0b1c --max-chars 8000
```

---

## 6. Safety & Operational Rules

1. **Read Operations First**: Prefer inspecting issues (`jira get`), PR diffs (`bitbucket diff-pr`), pages (`confluence get`), or CI builds (`bitrise get-build`) before taking modifying actions.
2. **User Confirmation**: Confirm with the user before performing modifying actions like updating issues or posting comments unless explicitly asked.
3. **No Unwanted Test Writes**: Never run write commands against live production instances for test purposes.
