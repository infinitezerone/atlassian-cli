# Bitbucket Command Reference

Load this file when a task requires any Bitbucket PR operation beyond the quick-reference in SKILL.md.

## List PRs & Create PR
```bash
# 1. Global personal PR dashboard (no repo needed; defaults to author/OPEN)
atlassian-cli bitbucket list-prs                          # list open PRs created by me across all repos
atlassian-cli bitbucket list-prs --role reviewer          # list open PRs requesting my review
atlassian-cli bitbucket list-prs --state MERGED           # list recently merged PRs

# 2. List PRs by project & repository (accepts repo URL)
atlassian-cli bitbucket list-prs --project PROJ --repo my-repo --state OPEN
atlassian-cli bitbucket list-prs --url https://gitpub.example.com/projects/PROJ/repos/my-repo

# 3. Create Pull Request (accepts --url or --project/--repo, auto-loads web default reviewers, supports extra --reviewers)
atlassian-cli bitbucket create-pr --project PROJ --repo my-repo --title "Fix login timeout" --from feature/login-fix --to main --reviewers "john.doe, jane.smith" --confirm
atlassian-cli bitbucket create-pr --url https://gitpub.example.com/projects/PROJ/repos/my-repo --title "Fix login timeout" --from feature/login-fix --to main --confirm

# 4. Update PR Metadata (Title, Description, or Reviewers; auto-fetches version and merges existing fields)
atlassian-cli bitbucket update-pr 100 --title "Updated PR Title" --confirm
atlassian-cli bitbucket update-pr 100 --description "Revised description with testing steps" --confirm
atlassian-cli bitbucket update-pr https://gitpub.../pull-requests/100 --title "New Title" --description "New Desc" --reviewers "john.doe, jane.smith" --confirm
```

## Inspect PR Details & Code Diffs (Token-Budget Friendly)
```bash
# Get PR overview (accepts PR ID or direct webpage URL)
atlassian-cli bitbucket get-pr 100

# View changed files overview only (--stat, 0-diff body, ultra token saver!)
atlassian-cli bitbucket diff-pr 100 --stat

# Precise single-file diff review (avoiding huge diff context dumps)
atlassian-cli bitbucket diff-pr 100 --file "src/main/java/App.java"

# View PR code diff with line budget limit & pagination
atlassian-cli bitbucket diff-pr 100 --max-lines 500 --offset 0

# View PR comment tree & discussions
atlassian-cli bitbucket comments-pr 100
```

## Post Code Review Comments & Approve PR
```bash
# Resolve Bitbucket user mention_syntax (@"username") before mentioning users in PR comments
# Bitbucket Server / Data Center requires double quotes around usernames containing '-', '.', etc.
atlassian-cli bitbucket user "john"                      # returns mention_syntax: @"john.doe"

# General PR comment
atlassian-cli bitbucket comment-pr 100 --text "LGTM, overall architecture is clean. cc @\"john.doe\"" --confirm

# Precise file line inline comment (Code Review)
atlassian-cli bitbucket comment-pr 100 \
  --text "Consider adding null-check here to prevent NullPointerException" \
  --file "src/main/java/App.java" \
  --line 42

# Approve Pull Request
atlassian-cli bitbucket approve-pr 100 --confirm
```

## Update & Delete PR Comments
```bash
# First check comment ID using comments-pr (returns comments[].id and comments[].version):
atlassian-cli bitbucket comments-pr 100

# Update PR comment (auto-fetches latest version for optimistic lock if --version is omitted)
atlassian-cli bitbucket comment-update-pr 100 10001 --text "Revised review feedback" --confirm
atlassian-cli bitbucket comment-update-pr https://gitpub.../pull-requests/100 10001 "Revised feedback" --confirm

# Delete PR comment (auto-fetches latest version if --version is omitted)
atlassian-cli bitbucket comment-delete-pr 100 10001 --confirm
atlassian-cli bitbucket comment-delete-pr https://gitpub.../pull-requests/100 10001 --confirm
```
