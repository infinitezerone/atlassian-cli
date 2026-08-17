# Confluence Command Reference

Load this file when a task requires any Confluence page operation beyond the quick-reference in SKILL.md.

## Search Documentation & Spaces (CQL)
```bash
# Full-text search with pagination
atlassian-cli confluence search "Architecture Design" --limit 5 --start-at 10

# Title-only exact search (lightweight, zero false positives)
atlassian-cli confluence search "Release Plan" --title-only --space PROJ

# List or search accessible Confluence spaces
atlassian-cli confluence spaces --query "Mobile"
```

## Inspect Page, Child Tree & Attachments (Lightweight / 0-body)
```bash
# Quick title & metadata check (0 body fetched, saves tokens)
atlassian-cli confluence get 12345678 --title-only

# List direct child pages under a parent topic (returns IDs & titles)
atlassian-cli confluence children 12345678 --limit 20

# List attachments on a page
atlassian-cli confluence attachments 12345678

# Download attachment locally with PAT authentication (accepts ID or filename)
atlassian-cli confluence attachment-download 12345678 spec.pdf --output ./downloads/spec.pdf

# Upload local file to a Confluence page
atlassian-cli confluence attach 12345678 ./spec.pdf --comment "Updated architecture spec" --confirm
```

## Fetch Page Body
```bash
# Fetch page text (default 8000 chars, accepts Page ID or browser URL)
atlassian-cli confluence get 12345678

# Paginate long documents using --offset
atlassian-cli confluence get 12345678 --offset 8000 --max-chars 8000

# Fetch raw HTML storage format
atlassian-cli confluence get 12345678 --raw
```

## Create & Update Pages (Macro-enabled)
```bash
# Create page (supports Date <time> pills & Jira issue cards)
atlassian-cli confluence create --space PROJ --title "Release Notes 6.2.0" \
  --body "Release date: <time datetime=\"2026-08-13\"/>\nRelated ticket: <ac:structured-macro ac:name=\"jira\"><ac:parameter ac:name=\"key\">PROJ-123</ac:parameter></ac:structured-macro>"

# Update page: find & replace (strictly requires exact 1 occurrence to avoid corrupting text)
atlassian-cli confluence update 12345678 --find "v6.1.0" --replace "v6.2.0" --confirm

# Update page: append content at bottom / prepend at top
atlassian-cli confluence update 12345678 --append "\n## Appendix\nAdditional deployment steps." --confirm
```
