#!/usr/bin/env bash
# Cut a release: bump every place the version really lives, commit, tag, push.
#
#   scripts/release.sh 0.2.0              # checks, plan, confirm, push
#   scripts/release.sh 0.2.0 --dry-run    # print the plan, write nothing
#   scripts/release.sh 0.2.0 --yes        # no confirmation prompt
#   scripts/release.sh 0.2.0 --skip-checks  # skip the local CI run
#
# Pushing the tag is the point of no return: `release.yml` and `docker.yml`
# both fire on `v*` and publish. Everything before the push is reversible, so
# the script does all of it first, shows the diff, and only then asks.
set -euo pipefail

cd "$(dirname "$0")/.."

die() { printf '\033[31merror\033[0m  %s\n' "$1" >&2; exit 1; }
note() { printf '\033[1m%s\033[0m\n' "$1"; }

VERSION=""
DRY_RUN=0
ASSUME_YES=0
SKIP_CHECKS=0

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --yes|-y) ASSUME_YES=1 ;;
    --skip-checks) SKIP_CHECKS=1 ;;
    -*) die "unknown flag: $arg" ;;
    *)
      [[ -n "$VERSION" ]] && die "give exactly one version"
      VERSION="$arg"
      ;;
  esac
done

[[ -n "$VERSION" ]] || die "usage: scripts/release.sh <version> [--dry-run] [--yes] [--skip-checks]"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || die "'$VERSION' is not a semver version (expected X.Y.Z)"

TAG="v$VERSION"
CURRENT="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
[[ -n "$CURRENT" ]] || die "could not read the current version out of Cargo.toml"

# --- preconditions -----------------------------------------------------------
# Each one is a way a release goes out wrong, so none of them is a warning.

[[ "$VERSION" != "$CURRENT" ]] || die "the manifest is already at $VERSION"

# Sorting the two versions is enough to catch a typo that would go backwards
# ("0.1.4" after "0.2.0"), which no later step would notice.
LOWEST="$(printf '%s\n%s\n' "$CURRENT" "$VERSION" | sort -V | head -1)"
[[ "$LOWEST" == "$CURRENT" ]] || die "$VERSION is older than the current $CURRENT"

[[ -z "$(git status --porcelain)" ]] || die "the working tree has changes; commit or discard them first"

git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null \
  && die "tag $TAG already exists locally"
git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1 \
  && die "tag $TAG already exists on origin"

# A tag has to sit on a commit origin already has, or the release builds a
# revision nobody can check out. Comparing against the local `main` ref rather
# than the branch name keeps this working in a jj colocated workspace, where
# git HEAD is detached by design.
git fetch --quiet origin main
HEAD_SHA="$(git rev-parse HEAD)"
MAIN_SHA="$(git rev-parse refs/heads/main 2>/dev/null || echo none)"
ORIGIN_SHA="$(git rev-parse origin/main)"
[[ "$HEAD_SHA" == "$MAIN_SHA" ]] || die "HEAD is not at local main ($HEAD_SHA vs $MAIN_SHA)"
[[ "$HEAD_SHA" == "$ORIGIN_SHA" ]] || die "main is not in sync with origin/main; push or pull first"

if [[ $SKIP_CHECKS -eq 0 ]]; then
  note "── running the local CI (scripts/ci/local.sh), pass --skip-checks to skip"
  scripts/ci/local.sh || die "local CI failed; fix it or re-run with --skip-checks"
fi

# --- the bump ----------------------------------------------------------------
# Only the places where this number IS the project's version. Notably NOT
# GETTING-STARTED.md, whose "37 tools as of 0.1.3" is a statement about what
# 0.1.3 shipped: rewriting it to the new version asserts something nobody
# checked. It is reported at the end instead.

note "── $CURRENT -> $VERSION"

bump_file() {
  local file="$1" expr="$2"
  [[ -f "$file" ]] || die "expected $file to exist"
  if [[ $DRY_RUN -eq 1 ]]; then
    printf '  would edit  %s\n' "$file"
  else
    perl -0pi -e "$expr" "$file"
    printf '  edited      %s\n' "$file"
  fi
}

# Anchored to the first `version = "..."` so a dependency pinned at the same
# number is never touched.
bump_file Cargo.toml \
  "s/^version = \"\Q$CURRENT\E\"\$/version = \"$VERSION\"/m"
bump_file crates/telemaco/Cargo.toml \
  "s/^version = \"\Q$CURRENT\E\"\$/version = \"$VERSION\"/m"
# The published image tag shown as an example of the current release.
bump_file README.md \
  "s/albz222\/telemaco:\Q$CURRENT\E/albz222\/telemaco:$VERSION/g"

if [[ $DRY_RUN -eq 1 ]]; then
  printf '  would run   cargo update -w (refresh Cargo.lock)\n'
else
  cargo update -w --quiet || die "cargo update failed"
  printf '  refreshed   Cargo.lock\n'
fi

# The lockfile ships with the manifest in one commit, never in a follow-up.
if [[ $DRY_RUN -eq 0 ]]; then
  grep -q "^name = \"telemaco\"\$" Cargo.lock || die "Cargo.lock lost the telemaco entry"
fi

# --- plan --------------------------------------------------------------------

note "── what this will publish"
if [[ $DRY_RUN -eq 0 ]]; then
  git --no-pager diff --stat
  printf '\n'
fi
printf '  commit  chore(release): %s\n' "$VERSION"
printf '  tag     %s (annotated, on the new commit)\n' "$TAG"
printf '  push    main and %s to origin\n' "$TAG"
printf '\n  the tag starts release.yml (archives for 5 targets x 4 variants)\n'
printf '  and docker.yml (albz222/telemaco:%s and :latest)\n\n' "$VERSION"

if [[ $DRY_RUN -eq 1 ]]; then
  note "dry run: nothing was written"
  exit 0
fi

if [[ $ASSUME_YES -eq 0 ]]; then
  printf 'Push the release? [y/N] '
  read -r reply
  [[ "$reply" == "y" || "$reply" == "Y" ]] || {
    note "stopped. The working tree still holds the bump; `git checkout .` discards it."
    exit 1
  }
fi

# --- commit, tag, push -------------------------------------------------------

git add Cargo.toml Cargo.lock crates/telemaco/Cargo.toml README.md
git commit --quiet -m "chore(release): $VERSION"
git tag -a "$TAG" -m "$TAG"
git push --quiet origin "HEAD:refs/heads/main"
git push --quiet origin "refs/tags/$TAG"

note "── pushed $TAG"
printf '  watch    gh run list --limit 5\n'
printf '  release  https://github.com/AlbertoBarrago/telemaco/releases/tag/%s\n\n' "$TAG"

note "── still manual"
printf '  Homebrew: the tap (AlbertoBarrago/homebrew-telemaco) pins url + sha256\n'
printf '  per formula, and the macOS archives only exist once release.yml has\n'
printf '  finished. Update Formula/telemaco.rb and Formula/telemaco-stealth.rb after.\n'
REMAINING="$(grep -rn "$CURRENT" --include='*.md' . 2>/dev/null | grep -v '^./target' | grep -v vendor || true)"
if [[ -n "$REMAINING" ]]; then
  printf '  Docs still naming %s, left alone on purpose (check if they should change):\n' "$CURRENT"
  printf '%s\n' "$REMAINING" | sed 's/^/    /'
fi
