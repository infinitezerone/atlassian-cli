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
# General PR comment
atlassian-cli bitbucket comment-pr 100 --text "LGTM, overall architecture is clean." --confirm

# Precise file line inline comment (Code Review)
atlassian-cli bitbucket comment-pr 100 \
  --text "Consider adding null-check here to prevent NullPointerException" \
  --file "src/main/java/App.java" \
  --line 42

# Approve Pull Request
atlassian-cli bitbucket approve-pr 100 --confirm
```
