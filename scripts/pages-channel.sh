#!/usr/bin/env bash
# Puts a run's firmware manifests on the <product>-<channel> branches that the Pages site reads, a run
# replaces only what it built. The release channel starts over from the latest LIVI release's
# manifests once that is newer. Starts the pages workflow when a branch changed.
#
# usage: pages-channel.sh <product> "<channel>..." "<target>..."
set -euo pipefail
PRODUCT=${1:?usage: pages-channel.sh <product> <channels> <targets>}
CHANNELS=${2:?}
TARGETS=${3:-}
REPO=${GITHUB_REPOSITORY:?}
SEEDED="assets/$PRODUCT"
[ "$PRODUCT" = livi-link ] && SEEDED+=" assets/livi-link-provision"

here=$(git rev-parse HEAD)
tag='' published=''
if latest=$(gh api "repos/$REPO/releases/latest" -q '.tag_name + " " + .published_at' 2>/dev/null); then
  read -r tag published <<<"$latest"
fi

seed() {
  git -C "$1" rm -rq --ignore-unmatch .
  for p in $SEEDED; do
    git -C "$1" checkout "$2" -- "$p" 2>/dev/null || true
  done
}

changed=
for channel in $CHANNELS; do
  branch=$PRODUCT-$channel
  work=${RUNNER_TEMP:-/tmp}/$branch
  base=
  if git fetch -q --depth 1 origin "$branch" 2>/dev/null; then
    git worktree add -q -B "$branch" "$work" FETCH_HEAD
    if [ "$channel" = release ] && [ -n "$tag" ]; then
      head=$(gh api "repos/$REPO/commits/$branch" -q .commit.committer.date)
      [[ "$head" > "$published" ]] || base=tag
    fi
  else
    git worktree add -q --orphan -b "$branch" "$work"
    base=here
    [ "$channel" = release ] && [ -n "$tag" ] && base=tag
  fi
  case $base in
    here) seed "$work" "$here" ;;
    tag)
      git fetch -q --depth 1 origin "refs/tags/$tag:refs/tags/$tag"
      seed "$work" "refs/tags/$tag" ;;
  esac

  for t in $TARGETS; do
    [ -f "assets/$PRODUCT/$t/firmware.json" ] || continue
    mkdir -p "$work/assets/$PRODUCT/$t"
    cp "assets/$PRODUCT/$t/firmware.json" "$work/assets/$PRODUCT/$t/"
  done
  if [ "$PRODUCT" = livi-link ] && compgen -G 'provision/livi-link-provision-*' >/dev/null; then
    mkdir -p "$work/assets/livi-link-provision"
    cp assets/livi-link-provision/*.json "$work/assets/livi-link-provision/"
  fi

  git -C "$work" add -A .
  if git -C "$work" diff --cached --quiet; then
    echo "$channel on Pages already has these files"
    continue
  fi
  git -C "$work" commit -qm "chore($PRODUCT): $channel from ${GITHUB_REF_NAME:-local} (${here::8})"
  git -C "$work" push -q origin "$branch"
  changed=1
done
[ -z "$changed" ] || gh workflow run pages.yml --repo "$REPO" --ref main
