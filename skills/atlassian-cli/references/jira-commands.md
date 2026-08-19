# Jira Command Reference

Load this file when a task requires any Jira operation beyond the quick-reference in SKILL.md. `atlassian-cli` accepts both **Issue Keys** (e.g., `PROJ-123`) and **direct browser webpage URLs** (e.g., `https://jira.example.com/browse/PROJ-123`).

## Read Issue & Discussion Comments
```bash
atlassian-cli jira get PROJ-123
# Control returned comments count (default 10, set 0 to hide comments)
atlassian-cli jira get PROJ-123 --comments-limit 5
```

## Search Issues (JQL)
```bash
# Default search: queries unclosed issues assigned to current user (zero-args!)
atlassian-cli jira search

# Custom JQL query
atlassian-cli jira search "assignee = currentUser() AND status != Closed" --limit 10
# Restrict returned fields to save tokens (comma-separated, '-' excludes)
atlassian-cli jira search "project = PROJ" --fields summary,status,assignee
# Paginate with --start-at (a hint field shows the next page offset)
atlassian-cli jira search "project = PROJ" --limit 50 --start-at 0
```

**Before composing a JQL query, resolve real field names & values — never guess:**

```bash
# List all available JQL fields & functions (official autocompletedata API)
atlassian-cli jira suggest-fields

# Resolve candidate values for a field (users, projects, statuses, versions...)
atlassian-cli jira suggest-values --field assignee --query "john"
```

- Use the `value` from `suggest-values` results inside the JQL string.
- Malformed JQL (unbalanced parens, unclosed quotes, empty query) is rejected **locally** with `PARAM_INVALID` (exit 2) plus a fix suggestion, before any request is sent.

## Add / Edit / Delete Comments
```bash
# Add comment (supports --body / --text or positional argument)
atlassian-cli jira comment PROJ-123 --body "Analysis completed. Pending code review." --confirm
atlassian-cli jira comment PROJ-123 "Analysis completed." --confirm

# Get the comment id first (jira get returns comments[].id), then:
atlassian-cli jira comment-update PROJ-123 10001 --body "Revised comment text" --confirm
atlassian-cli jira comment-delete PROJ-123 10001 --confirm
```

## Transition Issue Status
```bash
atlassian-cli jira transition PROJ-123 "In Progress" --confirm
atlassian-cli jira transition PROJ-123 "Done" --confirm
```

## Create / Update / Bulk Create / Clone
```bash
# Create Issue (supports custom fields via --custom and --custom-json)
atlassian-cli jira create --project PROJ --summary "Fix login timeout bug" --issue-type Bug --assignee john.doe --priority High --custom "customfield_10020=5" --confirm

# Update Issue fields
atlassian-cli jira update PROJ-123 --summary "Updated title" --priority Medium --labels "backend,urgent" --custom "customfield_10010=PROJ-10" --confirm

# Bulk create (one request, N issues; shared project/type/priority/labels/custom template)
atlassian-cli jira bulk-create --project PROJ --summaries "task A,task B,task C" --issue-type Bug --priority High --confirm

# Clone an issue (copies business fields, resets status/assignee; --link adds Cloners, --comment traces on source)
atlassian-cli jira clone PROJ-123 --summary "CLONE - recurring task" --link --confirm
```

## Assign Issue & User Search
```bash
# Search assignable users for an issue (matches webpage autocomplete)
atlassian-cli jira assignable-users PROJ-123 "john"

# Look up username & mention_syntax ([~username])
atlassian-cli jira user "john"

# Assign issue (auto-sanitizes [~username] or @{username} input)
atlassian-cli jira assign PROJ-123 john.doe --confirm

# Unassign issue (supports unassigned, none, or -1)
atlassian-cli jira assign PROJ-123 unassigned --confirm
```

## Mentions (@) — Always resolve before writing

```bash
# Resolve the real mention_syntax for a person FIRST (never hand-write @names)
atlassian-cli jira user "john"          # returns mention_syntax: [~john.doe]

# Then use it in comments / descriptions
atlassian-cli jira comment PROJ-123 "Please review, thanks [~john.doe]" --confirm
```

- Broken mention syntax (`[~john doe]`, `[~]`, unclosed `[~...`) is rejected with `PARAM_INVALID` (exit 2) and a pointer to `jira user`.
- Bare `@` text is left untouched (it's plain text in Jira, not a mention).

## Transitions, Links, Attachments & Fields
```bash
# Introspect system and custom field metadata (translate customfield_xxx to human names)
atlassian-cli jira fields --query "Sprint"
atlassian-cli jira fields --custom-only

# List all visible projects (with --query filter)
atlassian-cli jira projects
atlassian-cli jira projects --query "mobile"

# List available issue types for a project (avoids guessing --issue-type)
atlassian-cli jira issue-types --project PROJ

# Inspect all available status transitions (avoid guessing transition names!)
atlassian-cli jira transitions PROJ-123

# Link two issues together (supports Relates, Blocks, Cloners, Duplicate)
atlassian-cli jira link PROJ-123 PROJ-456 --type "Relates" --comment "Related backend task" --confirm

# List attachments on an issue
atlassian-cli jira attachments PROJ-123

# Download attachment locally with PAT authentication (accepts ID or filename)
atlassian-cli jira attachment-download PROJ-123 crash.log --output ./downloads/crash.log

# Upload local file to an issue
atlassian-cli jira attach PROJ-123 ./crash.log --confirm

# Delete an attachment (accepts ID or filename)
atlassian-cli jira attachment-delete PROJ-123 crash.log --confirm
```

## Watchers (View / Add / Remove)
```bash
# View watchers of an issue
atlassian-cli jira watchers PROJ-123

# Add a watcher (--confirm required)
atlassian-cli jira watchers PROJ-123 --add john.doe --confirm

# Remove a watcher (--confirm required)
atlassian-cli jira watchers PROJ-123 --remove john.doe --confirm
```

## Worklog & Time Tracking
```bash
# Log time spent on an issue (supports "2h 30m", "1d", "45m", --comment, --started)
# time_spent units w/d/h/m validated (no repeats); --started accepts
# "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS" (month 1-12, day 1-31)
atlassian-cli jira worklog-add PROJ-123 "2h 30m" --comment "Completed code review and unit tests" --started "2026-08-15T09:30:00" --confirm

# List worklog entries on an issue
atlassian-cli jira worklog-list PROJ-123

# Delete a specific worklog entry
atlassian-cli jira worklog-delete PROJ-123 7858155 --confirm
```
