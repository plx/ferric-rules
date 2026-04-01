#!/usr/bin/env bash
set -euo pipefail

# Finds the next actionable open GitHub issue matching the given labels.
#
# Emits one of three outputs:
#   1. A detailed issue report (if an unblocked issue exists)
#   2. A brief "no open issues" message
#   3. An "all blocked" message with summary tables
#
# Usage: find-next-matching-issue.sh <label1,label2,...>
# Example: find-next-matching-issue.sh golang-binding,remediation

LABELS="${1:?Usage: $0 <label1,label2,...>}"
REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
LABELS_DISPLAY="${LABELS//,/, }"

# Build --label flags for gh issue list
LABEL_ARGS=()
IFS=',' read -ra LABEL_ARRAY <<< "$LABELS"
for label in "${LABEL_ARRAY[@]}"; do
    LABEL_ARGS+=(--label "$label")
done

# Temp directory for jq filter files
JQTMP=$(mktemp -d)
trap 'rm -rf "$JQTMP"' EXIT

# ── Fetch data ──────────────────────────────────────────────────────────────

# Matched open issues (label-filtered, with body for dependency parsing)
MATCHED_JSON=$(gh issue list --repo "$REPO" "${LABEL_ARGS[@]}" \
    --state open --json number,title,labels,body --limit 200)

MATCH_COUNT=$(echo "$MATCHED_JSON" | jq 'length')

# Case 2: No open issues
if [[ "$MATCH_COUNT" -eq 0 ]]; then
    echo "There are no open issues matching ${LABELS_DISPLAY}."
    exit 0
fi

# All open issues (for complete dependency resolution across issue sets)
ALL_OPEN_JSON=$(gh issue list --repo "$REPO" --state open \
    --json number,title,labels,body --limit 500)

# ── Analysis: determine case and collect relevant data ──────────────────────

cat > "$JQTMP/analysis.jq" <<'JQEOF'
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

# Given an issue object as input, returns the array of open issue numbers
# that block it.  id_map and open_set are passed as filter arguments.
def open_blockers(id_map; open_set):
  (try ((.body // "") | capture("ID: `(?<id>[^`]+)`") | .id) catch null) as $own_id |
  ([(.body // "") | scan("GOB-[0-9]+")] | map(select(. != $own_id)) | unique) as $dep_ids |
  [$dep_ids[] | id_map[.] // null | select(. != null)] |
  map(select(open_set[tostring] == true));

($filter_labels | split(",")) as $filter_set |

# Build tracking_id -> issue-number map from ALL open issues
(reduce $all_open[] as $issue ({};
  (try (($issue.body // "") | capture("ID: `(?<id>[^`]+)`") | .id) catch null) as $tid |
  if $tid then . + {($tid): $issue.number} else . end
)) as $id_map |

# Open issue numbers as a lookup set (string-keyed for jq object lookup)
(reduce $all_open[].number as $n ({}; . + {($n | tostring): true})) as $open_set |

# ── Process matched issues ──

[$matched[] |
  (.labels | map(.name)) as $all_labels |
  ($all_labels | map(select(test("^p[0-9]+$"))) | .[0] // "-") as $priority |
  ($all_labels | map(select(startswith("size/"))) | .[0] // "-") as $size |
  ([$all_labels[] | select(
    . as $l |
    ($filter_set | index($l)) == null and
    ($l | test("^p[0-9]+$") | not) and
    ($l | startswith("size/") | not)
  )] | sort | join(", ")) as $other |
  open_blockers($id_map; $open_set) as $ob |
  {
    number,
    title,
    priority:          $priority,
    priority_rank:     ($priority | priority_rank),
    size:              $size,
    size_display:      (if $size == "-" then "-"
                        else ($size | ltrimstr("size/") | ascii_upcase) end),
    size_rank:         ($size | size_rank),
    blocked:           ($ob | length > 0),
    blocked_by:        $ob,
    blocked_by_display:(if ($ob | length) == 0 then ""
                        else [$ob[] | "#\(.)"] | join(", ") end),
    other:             $other
  }
] | sort_by(.priority_rank, .size_rank, .number) as $sorted |

[$sorted[] | select(.blocked | not)] as $unblocked |

if ($unblocked | length) > 0 then
  # Case 1: at least one unblocked issue — return the best one
  { case_num: 1, next_issue: $unblocked[0].number }
else
  # Case 3: all matched issues are blocked
  ([$sorted[].blocked_by[]] | unique) as $blocker_nums |

  # Build blocker details from all_open data
  [$all_open[] |
    select(.number as $n | $blocker_nums | index($n) != null) |
    (.labels | map(.name)) as $all_labels |
    ($all_labels | map(select(test("^p[0-9]+$"))) | .[0] // "-") as $priority |
    ($all_labels | map(select(startswith("size/"))) | .[0] // "-") as $size |
    open_blockers($id_map; $open_set) as $ob |
    {
      number,
      title,
      priority:        $priority,
      priority_rank:   ($priority | priority_rank),
      size:            $size,
      size_display:    (if $size == "-" then "-"
                        else ($size | ltrimstr("size/") | ascii_upcase) end),
      size_rank:       ($size | size_rank),
      blocked_display: (if ($ob | length > 0) then "true" else "false" end),
      labels_display:  ($all_labels | sort | join(", "))
    }
  ] | sort_by(.priority_rank, .size_rank, .number) as $blockers |

  {
    case_num: 3,
    count:    ($sorted | length),
    issues:   $sorted,
    blockers: $blockers
  }
end
JQEOF

ANALYSIS=$(jq -n \
    --argjson matched "$MATCHED_JSON" \
    --argjson all_open "$ALL_OPEN_JSON" \
    --arg filter_labels "$LABELS" \
    -f "$JQTMP/analysis.jq")

CASE_NUM=$(echo "$ANALYSIS" | jq -r '.case_num')

# ── Case 1: Unblocked issue found — emit detailed report ───────────────────

if [[ "$CASE_NUM" -eq 1 ]]; then
    NEXT_NUM=$(echo "$ANALYSIS" | jq -r '.next_issue')

    ISSUE_FULL=$(gh issue view "$NEXT_NUM" --repo "$REPO" \
        --json number,url,title,author,createdAt,updatedAt,labels,milestone,body,comments)

    cat > "$JQTMP/report.jq" <<'JQEOF'
"We have identified the following open issue matching \($labels) as open and unblocked:",
"",
"`````````md",
"---",
"id: #\(.number)",
"url: \(.url)",
"title: \(.title)",
"author: \(.author.login)",
"created-at: \(.createdAt)",
"updated-at: \(.updatedAt)",
"labels: \([.labels[].name] | sort | join(", "))",
"milestone: \(.milestone.title // "none")",
"---",
"",
.body,
"",
"`````````",
(if (.comments | length) > 0 then
    "",
    "Issue comments:",
    "",
    (.comments | sort_by(.createdAt)[] |
        "`````````md",
        "---",
        "id: \(.id)",
        "url: \(.url)",
        "author: \(.author.login)",
        "created-at: \(.createdAt)",
        "updated-at: \(.updatedAt)",
        "---",
        "",
        .body,
        "",
        "`````````"
    )
else empty end)
JQEOF

    echo "$ISSUE_FULL" | jq -r --arg labels "$LABELS_DISPLAY" -f "$JQTMP/report.jq"
    exit 0
fi

# ── Case 3: All blocked — emit summary tables ──────────────────────────────

cat > "$JQTMP/tables.jq" <<'JQEOF'
"We found \(.count) open issues matching \($labels), but all were blocked by other issues.",
"",
"Here are the open issues we found:",
"",
"| # | Title | Priority | Size | Other Labels | Blocked By |",
"|---|-------|----------|------|--------------|------------|",
(.issues[] |
    "| #\(.number) | \(.title) | \(.priority) | \(.size_display) | \(.other) | \(.blocked_by_display) |"),
"",
"Here are details for the blocking issues:",
"",
"| # | Title | Priority | Size | Labels | Blocked? |",
"|---|-------|----------|------|--------|----------|",
(.blockers[] |
    "| #\(.number) | \(.title) | \(.priority) | \(.size_display) | \(.labels_display) | \(.blocked_display) |")
JQEOF

echo "$ANALYSIS" | jq -r --arg labels "$LABELS_DISPLAY" -f "$JQTMP/tables.jq"
