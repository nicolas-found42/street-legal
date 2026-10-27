#!/bin/sh
# Make a git worktree runnable against the main checkout's local-only inputs.
#
# stock/, reference/ and re/out/ are git-ignored and live only in the main
# checkout. This links them READ-ONLY by convention: stock/ and reference/ are
# symlinked whole; re/out/ becomes a worktree-local directory whose entries are
# symlinks to the main checkout's entries, except that the directories named in
# LOCAL_OUT (default: boot menu acceptance) are real worktree-local directories.
# Their fixtures are linked, but run directories and indexes are kept local.
# Historical archives remain in the main checkout; inspect them explicitly
# with --runs-root instead of making new run writes follow a shared symlink.
#
# Usage: tools/link-worktree.sh [MAIN_CHECKOUT]
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
main=${1:-$(git -C "$here" worktree list --porcelain | sed -n '1s/^worktree //p')}
[ "$here" != "$main" ] || { echo "refusing to run in the main checkout" >&2; exit 2; }
for d in stock reference; do
  [ -e "$main/$d" ] || continue
  [ -e "$here/$d" ] || [ -L "$here/$d" ] || ln -s "$main/$d" "$here/$d"
done
[ ! -L "$here/re" ] && [ ! -L "$here/re/out" ] || { echo "refusing shared output ancestor: $here/re/out" >&2; exit 2; }
mkdir -p "$here/re/out"
local_out=${LOCAL_OUT:-boot menu acceptance}
for e in "$main"/re/out/*; do
  [ -e "$e" ] || continue
  n=$(basename "$e")
  case " $local_out " in
    *" $n "*)
      [ ! -L "$here/re/out/$n" ] || { echo "refusing shared output directory: $here/re/out/$n" >&2; exit 2; }
      mkdir -p "$here/re/out/$n"
      for mutable in runs test-runs inputs-store runs-index.jsonl; do
        [ ! -L "$here/re/out/$n/$mutable" ] || { echo "refusing shared output path: $here/re/out/$n/$mutable" >&2; exit 2; }
      done
      mkdir -p "$here/re/out/$n/runs" "$here/re/out/$n/test-runs" "$here/re/out/$n/inputs-store"
      for s in "$e"/*; do
        [ -e "$s" ] || continue
        sn=$(basename "$s")
        case "$sn" in runs|test-runs|inputs-store|runs-index.jsonl|runs-index.jsonl.*) continue ;; esac
        [ -e "$here/re/out/$n/$sn" ] || [ -L "$here/re/out/$n/$sn" ] || ln -s "$s" "$here/re/out/$n/$sn"
      done ;;
    *) [ -e "$here/re/out/$n" ] || [ -L "$here/re/out/$n" ] || ln -s "$e" "$here/re/out/$n" ;;
  esac
done
echo "linked $here -> $main (local: $local_out)"
