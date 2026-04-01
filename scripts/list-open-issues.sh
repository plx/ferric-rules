#!/usr/bin/env bash
set -euo pipefail

# Lists open GitHub issues matching the given labels as a markdown table.
#
# Usage: list-open-issues.sh <label1,label2,...>
# Example: list-open-issues.sh golang-binding,remediation

LABELS="${1:?Usage: $0 <label1,label2,...>}"
REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
LABELS_DISPLAY="${LABELS//,/, }"

# Build --label flags for gh issue list
LABEL_ARGS=()
IFS=',' read -ra LABEL_ARRAY <<< "$LABELS"
for label in "${LABEL_ARRAY[@]}"; do
    LABEL_ARGS+=(--label "$label")
done

# Fetch matching open issues
ISSUES_JSON=$(gh issue list --repo "$REPO" "${LABEL_ARGS[@]}" \
    --state open --json number,title,labels,url --limit 200)

COUNT=$(echo "$ISSUES_JSON" | jq 'length')

if [[ "$COUNT" -eq 0 ]]; then
    echo "No open issues matching ${LABELS_DISPLAY}."
    exit 0
fi

# Render as a markdown table
echo "$ISSUES_JSON" | jq -r --arg labels "$LABELS_DISPLAY" '

def priority_rank:
  if   . == "p0" then 0
  elif . == "p1" then 1
  elif . == "p2" then 2
  elif . == "p3" then 3
  elif . == "p4" then 4
  else 99 end;

def size_rank:
  if   . == "size/xs" then 0
  elif . == "size/s"  then 1
  elif . == "size/m"  then 2
  elif . == "size/l"  then 3
  elif . == "size/xl" then 4
  else 99 end;

[.[] |
  (.labels | map(.name)) as $all_labels |
  ($all_labels | map(select(test("^p[0-9]+$"))) | .[0] // "-") as $priority |
  ($all_labels | map(select(startswith("size/"))) | .[0] // "-") as $size |
  {
    number,
    title,
    url,
    priority:      $priority,
    priority_rank: ($priority | priority_rank),
    size:          $size,
    size_display:  (if $size == "-" then "-"
                    else ($size | ltrimstr("size/") | ascii_upcase) end),
    size_rank:     ($size | size_rank),
    labels:        ($all_labels | sort | join(", "))
  }
] | sort_by(.priority_rank, .size_rank, .number) |

"\(length) open issues matching \($labels):",
"",
"| # | Title | Priority | Size | Labels |",
"|---|-------|----------|------|--------|",
(.[] |
  "| [#\(.number)](\(.url)) | \(.title) | \(.priority) | \(.size_display) | \(.labels) |")
'
