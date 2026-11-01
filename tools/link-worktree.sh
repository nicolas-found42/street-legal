#!/bin/sh
# Make a git worktree runnable against the main checkout's local-only inputs.
#
# stock/, reference/ and re/out/ are git-ignored and live only in the main
# checkout. This links them READ-ONLY by convention: stock/ and reference/ are
# symlinked whole; re/out/ becomes a worktree-local directory whose entries are
# symlinks to the main checkout's entries, except that the directories named in
# LOCAL_OUT (default: boot) are real, worktree-local directories whose own
# entries are linked the same way. Runs a worktree starts therefore write into
# its own re/out/boot/..., never into the main checkout.
#
# Usage: tools/link-worktree.sh [MAIN_CHECKOUT]
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
main=${1:-$(git -C "$here" worktree list --porcelain | sed -n '1s/^worktree //p')}
[ "$here" != "$main" ] || { echo "refusing to run in the main checkout" >&2; exit 2; }
for d in stock reference; do
  [ -e "$main/$d" ] || continue
  [ -e "$here/$d" ] || ln -s "$main/$d" "$here/$d"
done
mkdir -p "$here/re/out"
local_out=${LOCAL_OUT:-boot}
for e in "$main"/re/out/*; do
  n=$(basename "$e")
  [ -e "$here/re/out/$n" ] || [ -L "$here/re/out/$n" ] && continue
  case " $local_out " in
    *" $n "*)
      mkdir -p "$here/re/out/$n"
      for s in "$e"/*; do
        [ -e "$s" ] || continue
        sn=$(basename "$s")
        [ -e "$here/re/out/$n/$sn" ] || [ -L "$here/re/out/$n/$sn" ] || ln -s "$s" "$here/re/out/$n/$sn"
      done ;;
    *) ln -s "$e" "$here/re/out/$n" ;;
  esac
done
echo "linked $here -> $main (local: $local_out)"
