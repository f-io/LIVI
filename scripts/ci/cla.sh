#!/bin/sh
# Signatures live as JSON lines on the "cla" branch, one per author and CLA.md revision.
#
# usage: cla.sh sign    (COMMENT_BODY, COMMENTER_ID, COMMENTER_LOGIN, COMMENT_ID, COMMENT_URL, COMMENTED_AT, PR)
#        cla.sh check   (PR, OWNER_ID, NOTIFY)
#        cla.sh stale
set -eu
: "${GITHUB_REPOSITORY:?}"
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
STORE=${CLA_STORE:-${RUNNER_TEMP:-/tmp}/cla}
BRANCH=cla
FILE=signatures.jsonl
PHRASE='I have read the CLA Document and I hereby sign the CLA'
DOC="https://github.com/$GITHUB_REPOSITORY/blob/main/CLA.md"
GUIDE="https://github.com/$GITHUB_REPOSITORY/blob/main/CONTRIBUTING.md#llms-and-generated-content"
STALE_HOURS=24
STALE_MARK='<!-- cla-stale -->'
AI='@(anthropic|openai)\.com$|^cursoragent@cursor\.com$|^aider@aider\.chat$|\+(copilot|copilot-swe-agent\[bot\]|devin-ai-integration\[bot\]|gemini-code-assist\[bot\]|chatgpt-codex-connector\[bot\])@users\.noreply\.github\.com$'

sha() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

store() {
  [ -e "$STORE/.git" ] && return
  if git -C "$ROOT" fetch -q origin "$BRANCH" 2>/dev/null; then
    git -C "$ROOT" worktree add -q -B "$BRANCH" "$STORE" FETCH_HEAD
  else
    git -C "$ROOT" worktree add -q --orphan -b "$BRANCH" "$STORE"
  fi
  git -C "$STORE" config user.name "github-actions[bot]"
  git -C "$STORE" config user.email "41898282+github-actions[bot]@users.noreply.github.com"
}

signed() {
  [ -f "$STORE/$FILE" ] || return 1
  jq -n -e --argjson id "$1" --arg cla "$2" \
    '[inputs | select(.id == $id and .cla_sha256 == $cla)] | length > 0' "$STORE/$FILE" >/dev/null
}

head_sha() {
  gh api "repos/$GITHUB_REPOSITORY/pulls/$PR" --jq .head.sha
}

sign() {
  body=$(printf '%s' "$COMMENT_BODY" | tr -s '[:space:]' ' ' | sed 's/^ //; s/ $//; s/\.$//')
  if [ "$body" != "$PHRASE" ]; then
    echo "not a signature"
    return
  fi
  store
  cla=$(sha "$ROOT/CLA.md")
  if signed "$COMMENTER_ID" "$cla"; then
    echo "$COMMENTER_LOGIN already signed this revision"
    return
  fi
  jq -cn \
    --argjson id "$COMMENTER_ID" \
    --arg login "$COMMENTER_LOGIN" \
    --argjson comment_id "$COMMENT_ID" \
    --arg comment_url "$COMMENT_URL" \
    --argjson pr "$PR" \
    --arg head_sha "$(head_sha)" \
    --arg signed_at "$COMMENTED_AT" \
    --arg cla_sha256 "$cla" \
    --arg body "$COMMENT_BODY" \
    '{id: $id, login: $login, signed_at: $signed_at, cla_sha256: $cla_sha256, pr: $pr, head_sha: $head_sha, comment_id: $comment_id, comment_url: $comment_url, body: $body}' \
    >> "$STORE/$FILE"
  git -C "$STORE" add "$FILE"
  git -C "$STORE" commit -q -m "chore(cla): $COMMENTER_LOGIN signs in #$PR"
  for _ in 1 2 3; do
    git -C "$STORE" push -q origin "HEAD:$BRANCH" && { echo "recorded $COMMENTER_LOGIN"; return; }
    git -C "$STORE" pull -q --rebase origin "$BRANCH"
  done
  echo "could not push the signature" >&2
  exit 1
}

authors() {
  # shellcheck disable=SC2016
  gh api graphql --paginate \
    -F owner="${GITHUB_REPOSITORY%/*}" -F name="${GITHUB_REPOSITORY#*/}" -F number="$PR" \
    -f query='query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          commits(first: 100, after: $endCursor) {
            pageInfo { hasNextPage endCursor }
            nodes { commit { authors(first: 20) { nodes { email user { databaseId login } } } } }
          }
        }
      }
    }' \
    --jq '.data.repository.pullRequest.commits.nodes[].commit.authors.nodes[]
      | [(.user.databaseId // 0), (.user.login // .email), (.email // "")] | @tsv'
}

check() {
  store
  cla=$(sha "$ROOT/CLA.md")
  head=$(head_sha)
  tab=$(printf '\t')
  list=$(authors | sort -u)
  missing=""
  ai=""
  while IFS=$tab read -r id who email; do
    [ -n "$id" ] || continue
    if printf '%s' "$email" | grep -qiE "$AI"; then
      ai="$ai $who"
      continue
    fi
    printf '%s' "$email" | grep -qE '\[bot\]@users\.noreply\.github\.com$' && continue
    [ "$id" = "$OWNER_ID" ] && continue
    if [ "$id" = 0 ] || ! signed "$id" "$cla"; then missing="$missing $who"; fi
  done <<EOF
$list
EOF
  desc=""
  [ -z "$ai" ] || desc="AI tool as author:$ai. "
  [ -z "$missing" ] || desc="${desc}Not signed:$missing"
  if [ -z "$desc" ]; then
    state=success
    desc="All commit authors have signed the CLA"
  else
    state=failure
  fi
  gh api -X POST "repos/$GITHUB_REPOSITORY/statuses/$head" \
    -f state="$state" -f context=CLA -f target_url="$DOC" \
    -f description="$(printf '%s' "$desc" | cut -c1-140)" >/dev/null
  echo "$desc"
  if [ "$state" = success ] && closed_for_cla "$PR"; then
    gh pr reopen "$PR" --repo "$GITHUB_REPOSITORY"
    echo "reopened #$PR"
  fi
  [ "$state" = failure ] && [ "${NOTIFY:-false}" = true ] || return 0
  note=""
  if [ -n "$ai" ]; then
    note="AI tools cannot author or co-author a contribution, see [CONTRIBUTING.md]($GUIDE). Remove them as commit author and from \`Co-authored-by\`:$ai

"
  fi
  if [ -n "$missing" ]; then
    note="${note}Every commit author and co-author needs to accept the [Contributor License Agreement]($DOC) before this pull request can be merged. To accept it, post the following as a comment on this pull request:

\`\`\`
$PHRASE
\`\`\`

Not signed yet:$missing"
  fi
  gh pr comment "$PR" --repo "$GITHUB_REPOSITORY" --body "$note"
}

closed_for_cla() {
  [ "$(gh api "repos/$GITHUB_REPOSITORY/pulls/$1" --jq '"\(.state) \(.merged)"')" = "closed false" ] || return 1
  gh api --paginate "repos/$GITHUB_REPOSITORY/issues/$1/comments" \
    --jq '.[] | select(.user.login == "github-actions[bot]") | .body' | grep -qF "$STALE_MARK"
}

stale() {
  cutoff=$(( $(date -u +%s) - STALE_HOURS * 3600 ))
  gh api --paginate "repos/$GITHUB_REPOSITORY/pulls?state=open&per_page=100" \
    --jq '.[] | "\(.number) \(.created_at) \(.head.sha)"' |
  while read -r number created sha; do
    [ "$(date -u -d "$created" +%s)" -lt "$cutoff" ] || continue
    state=$(gh api "repos/$GITHUB_REPOSITORY/commits/$sha/status" \
      --jq '[.statuses[] | select(.context == "CLA")][0].state // ""' </dev/null)
    [ "$state" = failure ] || continue
    gh pr close "$number" --repo "$GITHUB_REPOSITORY" --comment "Closed because the [Contributor License Agreement]($DOC) was not signed within $STALE_HOURS hours. Once every commit author has posted the line below, this pull request opens again.

\`\`\`
$PHRASE
\`\`\`
$STALE_MARK" </dev/null
    echo "closed #$number"
  done
}

case "${1:-}" in
  sign) sign ;;
  check) check ;;
  stale) stale ;;
  *) sed -n '4,6s/^# //p' "$0" >&2; exit 2 ;;
esac
