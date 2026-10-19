#!/bin/bash
#
# extract-stock.sh — rebuild the clean stock install from the stock media.
#
# What it does:
#   Turns the user's stock media (the v2.2.1 MSI + Data1.cab under
#   reference/raw-stock/) into the clean stock install (default:
#   <repo-root>/stock/) without ever writing into reference/:
#     1. Creates a case-sensitive APFS (HFS+ fallback) scratch volume with
#        hdiutil, because 31 cab keys differ only by case while the boot
#        volume is case-insensitive APFS.
#     2. Extracts the cabinet payload flat with cabextract on that volume.
#     3. Resolves every file's installed path from the MSI Directory /
#        Component / File tables (msiinfo export): INSTALLDIR subtree, long
#        file names, component -> directory mapping.
#     4. Copies each cab key to its resolved path under the target.
#     5. Verifies: file count equals the File table row count (7,858);
#        every file's size equals File.FileSize; the tree is byte-identical
#        to an independent msiextract tree; SHA-256 of
#        StreetLegal_Redline.exe starts with 16b25cac and
#        StreetLegal_Redline.bak.exe equals 7d77466a...2e8edde.
#     6. Writes <target>/.slr-manifest (M7-04): SHA-256, size and relative
#        path of every file, tab separated, generated from the tree verified
#        in step 5. The game's startup check (crates/slr-io, module stock)
#        verifies the install against it, replacing the original's CD check.
#        The manifest lives inside the git-ignored target and is never
#        committed: it holds hashes derived from the original's files.
#
# Pipeline justification: cabextract plus our own msiinfo table-driven
# placement is the primary path (every placement is auditable against the
# MSI tables), while msiextract serves as the independent oracle for the
# byte-identical check — so both Homebrew tools required by M0-02 have a
# real, distinct role instead of one merely repeating the other.
#
# Prerequisites (Homebrew + system):
#   msitools (msiinfo, msiextract), cabextract, hdiutil, shasum, awk, diff.
#
# Usage:
#   tools/extract-stock.sh [--target DIR] [--media DIR|MSI.msi] [--force]
#   tools/extract-stock.sh --help
#
#   Defaults: target <repo-root>/stock/; media auto-discovered under
#   <repo-root>/reference/raw-stock/. Env overrides: SLR_STOCK_TARGET,
#   SLR_STOCK_MEDIA (explicit flags win). Refuses to run when the target
#   already exists and is non-empty unless --force is given, and never
#   deletes pre-existing content silently.
#
# Exit status: 0 on success; nonzero with a diagnostic on stderr on any
# mismatch (count, size, tree diff, hash) or tool failure.

set -euo pipefail

EXPECTED_COUNT=7858
EXPECTED_EXE_PREFIX="16b25cac"
EXPECTED_EXE="16b25cace5313208e5b6f8e149edea5261634f20dc7e45157f8dac0199bfcf24"
EXPECTED_BAK="7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${SLR_STOCK_TARGET:-$ROOT/stock}"
MEDIA_ARG="${SLR_STOCK_MEDIA:-}"
FORCE=0

usage() {
    sed -n '2,/^# Exit status/p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --target)
            [ $# -ge 2 ] || { echo "extract-stock: ERROR: --target needs a directory" >&2; exit 2; }
            TARGET="$2"; shift 2;;
        --media)
            [ $# -ge 2 ] || { echo "extract-stock: ERROR: --media needs a path" >&2; exit 2; }
            MEDIA_ARG="$2"; shift 2;;
        --force)
            FORCE=1; shift;;
        -h|--help)
            usage; exit 0;;
        *)
            echo "extract-stock: ERROR: unknown argument: $1 (see --help)" >&2; exit 2;;
    esac
done

die() {
    echo "extract-stock: ERROR: $*" >&2
    exit 1
}

log() {
    echo "extract-stock: $*"
}

for tool in msiinfo msiextract cabextract hdiutil shasum awk diff find; do
    command -v "$tool" >/dev/null 2>&1 \
        || die "required tool '$tool' not found (msitools + cabextract via Homebrew; hdiutil ships with macOS)"
done

# --- Locate the stock media (MSI + external cab). Read-only; never modified. ---
if [ -n "$MEDIA_ARG" ]; then
    case "$MEDIA_ARG" in
        *.msi)
            [ -f "$MEDIA_ARG" ] || die "media MSI not found: $MEDIA_ARG"
            MSI="$MEDIA_ARG";;
        *)
            [ -d "$MEDIA_ARG" ] || die "media directory not found: $MEDIA_ARG"
            MSI="$(find "$MEDIA_ARG" -iname '*.msi' -type f | head -n 1)"
            [ -n "$MSI" ] || die "no MSI found under: $MEDIA_ARG";;
    esac
else
    [ -d "$ROOT/reference/raw-stock" ] \
        || die "media not found: $ROOT/reference/raw-stock is missing (set --media or SLR_STOCK_MEDIA)"
    MSI="$(find "$ROOT/reference/raw-stock" -iname '*.msi' -type f)"
    [ -n "$MSI" ] || die "no MSI found under $ROOT/reference/raw-stock"
    [ "$(printf '%s\n' "$MSI" | wc -l)" -eq 1 ] \
        || die "expected exactly one MSI under reference/raw-stock, found several: $MSI"
fi
case "$MSI" in
    /*) :;;
    *) MSI="$(cd "$(dirname "$MSI")" && pwd)/$(basename "$MSI")";;
esac
MEDIADIR="$(dirname "$MSI")"
log "media MSI: $MSI"

# --- Target safety: never touch reference/, never delete silently. ---
case "$TARGET" in
    /*) :;;
    *) TARGET="$(cd "$(dirname "$TARGET")" 2>/dev/null && pwd)/$(basename "$TARGET")" \
        || die "cannot resolve target path: $TARGET";;
esac
[ -n "$TARGET" ] && [ "$TARGET" != "/" ] && [ "$TARGET" != "$ROOT" ] \
    || die "refusing to use target: $TARGET"
case "$TARGET" in
    "$ROOT"/reference/*|"$ROOT"/reference)
        die "refusing to write into reference/: $TARGET";;
esac
if [ -e "$TARGET" ] && [ -n "$(ls -A "$TARGET" 2>/dev/null)" ]; then
    [ "$FORCE" -eq 1 ] \
        || die "target already exists and is non-empty: $TARGET (re-run with --force to replace it)"
    log "--force: removing pre-existing target $TARGET"
    rm -rf "$TARGET"
fi
mkdir -p "$TARGET"
log "target: $TARGET"

# --- Case-sensitive scratch volume (31 cab keys differ only by case). ---
SCRATCH_PARENT="$(mktemp -d "${TMPDIR:-/tmp}/slr-stock.XXXXXX")"
SPARSE="$SCRATCH_PARENT/stock"
MNT="$SCRATCH_PARENT/mnt"
mkdir -p "$MNT"
cleanup() {
    # Always attempt the detach: mount(8) reports /private/var/... while
    # $TMPDIR may spell the same path as /var/... (symlink), so a mount
    # guard on the raw $MNT string can silently skip the detach.
    if [ -n "${MNT-}" ]; then
        # Retry: the first detach can race the kernel flush ("Resource
        # busy"); a short sleep plus retries detach cleanly.
        for _ in 1 2 3 4 5; do
            hdiutil detach "$MNT" >/dev/null 2>&1 && break
            sleep 2
        done
    fi
    if [ -n "${SCRATCH_PARENT-}" ] && [ -d "$SCRATCH_PARENT" ]; then
        rm -rf "$SCRATCH_PARENT"
    fi
}
trap cleanup EXIT INT TERM

attached=""
for fs in "Case-sensitive APFS" "Case-sensitive Journaled HFS+"; do
    if hdiutil create -size 2g -fs "$fs" -volname SLRStock -type SPARSE \
            -o "$SPARSE" >/dev/null 2>&1 \
        && hdiutil attach -nobrowse -mountpoint "$MNT" "$SPARSE.sparseimage" >/dev/null 2>&1; then
        attached="$fs"
        break
    fi
    rm -f "$SPARSE.sparseimage"
done
[ -n "$attached" ] || die "could not create+mount a case-sensitive scratch image (tried APFS, HFS+)"
log "scratch: $MNT ($attached)"

KEYDIR="$MNT/keys"      # flat cab payload, keyed by cab file name
TDIR="$MNT/tables"      # msiinfo table exports
REFDIR="$MNT/msiref"    # independent msiextract tree (verification oracle)
mkdir -p "$KEYDIR" "$TDIR" "$REFDIR"

# --- Export the MSI tables that drive placement. ---
for table in Directory Component File Media; do
    msiinfo export "$MSI" "$table" > "$TDIR/$table.idt" \
        || die "msiinfo export of table '$table' failed"
done

# External cabinet named by the Media table (fails loudly on embedded cabs).
CABNAME="$(awk -F'\t' '{ sub(/\r$/, "") } FNR > 3 && $4 !~ /^#/ && $4 != "" && $4 != "Cabinet" { print $4; exit }' "$TDIR/Media.idt")"
[ -n "$CABNAME" ] || die "no external cabinet in the MSI Media table (embedded cabinets are not supported)"
CAB="$MEDIADIR/$CABNAME"
[ -f "$CAB" ] || die "cabinet $CABNAME (from Media table) not found next to the MSI"
log "cabinet: $CAB"

# --- Primary path: cabextract the flat payload, then place files ourselves. ---
cabextract -q -d "$KEYDIR" "$CAB" \
    || die "cabextract failed on $CAB"

# Build key -> relpath/size from Directory/Component/File.
# IDT files are CRLF, tab-separated, with 3 header rows; names are
# short|long (long wins) or a bare long name; INSTALLDIR is the tree root.
awk -F'\t' '
    { sub(/\r$/, "") }
    FILENAME ~ /Directory\.idt$/ {
        if (FNR > 3) {
            dname = $3; sub(/^.*\|/, "", dname)
            dirparent[$1] = $2; dirname[$1] = dname
        }
        next
    }
    FILENAME ~ /Component\.idt$/ {
        if (FNR > 3) compdir[$1] = $3
        next
    }
    FILENAME ~ /File\.idt$/ {
        if (FNR <= 3) next
        key = $1; comp = $2; fname = $3; fsize = $4
        sub(/^.*\|/, "", fname)
        d = compdir[comp]
        if (d == "") { printf "NO_COMPONENT_DIR\t%s\n", key > "/dev/stderr"; failed = 1; next }
        n = 0; rel = fname
        while (d != "INSTALLDIR") {
            if (!(d in dirparent) || ++n > 1000) { printf "BAD_DIR_CHAIN\t%s\n", key > "/dev/stderr"; failed = 1; break }
            rel = dirname[d] "/" rel
            d = dirparent[d]
        }
        if (d == "INSTALLDIR") printf "%s\t%s\t%s\n", key, rel, fsize
        next
    }
    END { if (failed) exit 1 }
' "$TDIR/Directory.idt" "$TDIR/Component.idt" "$TDIR/File.idt" > "$TDIR/mapping.tsv" \
    || die "could not resolve installed paths from Directory/Component/File tables"
MAPPED="$(wc -l < "$TDIR/mapping.tsv" | tr -d ' ')"
[ "$MAPPED" -gt 0 ] || die "mapping is empty; table export or parse failed"

TAB="$(printf '\t')"
placed=0
while IFS="$TAB" read -r key rel size rest; do
    [ -n "${key-}" ] && [ -n "${rel-}" ] && [ -n "${size-}" ] && [ -z "${rest-}" ] \
        || die "malformed mapping row for key '${key-}'"
    src="$KEYDIR/$key"
    [ -f "$src" ] || die "cab key missing after extraction: $key"
    dest="$TARGET/$rel"
    mkdir -p "${dest%/*}" || die "cannot create directory for $rel"
    cp "$src" "$dest" || die "cannot place $key -> $rel"
    placed=$((placed + 1))
done < "$TDIR/mapping.tsv"
log "placed $placed files from $MAPPED File rows"

# --- Verify: count, per-file sizes, byte-identity vs msiextract, exe hashes. ---
actual="$(find "$TARGET" -type f | wc -l | tr -d ' ')"
[ "$MAPPED" -eq "$EXPECTED_COUNT" ] \
    || die "File table has $MAPPED rows, expected exactly $EXPECTED_COUNT"
[ "$actual" -eq "$EXPECTED_COUNT" ] \
    || die "target holds $actual files, expected exactly $EXPECTED_COUNT"
log "file count: $actual (matches File table, expected $EXPECTED_COUNT)"

mismatched=0
while IFS="$TAB" read -r key rel size rest; do
    have="$(wc -c < "$TARGET/$rel" | tr -d ' ')"
    if [ "$have" != "$size" ]; then
        echo "extract-stock: size mismatch: $rel (File.FileSize=$size, actual=$have)" >&2
        mismatched=$((mismatched + 1))
    fi
done < "$TDIR/mapping.tsv"
[ "$mismatched" -eq 0 ] \
    || die "$mismatched file(s) differ from File.FileSize"
log "all $actual file sizes equal File.FileSize"

msiextract --directory "$REFDIR" "$MSI" >/dev/null \
    || die "msiextract failed on $MSI"
REFEXE="$(find "$REFDIR" -name StreetLegal_Redline.exe -type f)"
[ -n "$REFEXE" ] && [ "$(printf '%s\n' "$REFEXE" | wc -l)" -eq 1 ] \
    || die "msiextract tree has no unique StreetLegal_Redline.exe root"
REFROOT="$(dirname "$REFEXE")"
if [ -n "$(diff -r "$REFROOT" "$TARGET")" ]; then
    diff -r "$REFROOT" "$TARGET" | head -n 20 >&2
    die "target differs from the msiextract tree (see above; full diff elided)"
fi
log "byte-identical to msiextract tree ($REFROOT)"

exe_hash="$(shasum -a 256 "$TARGET/StreetLegal_Redline.exe" | awk '{ print $1 }')"
bak_hash="$(shasum -a 256 "$TARGET/StreetLegal_Redline.bak.exe" | awk '{ print $1 }')"
log "SHA-256 StreetLegal_Redline.exe:     $exe_hash"
log "SHA-256 StreetLegal_Redline.bak.exe: $bak_hash"
case "$exe_hash" in
    "$EXPECTED_EXE") :;;
    "$EXPECTED_EXE_PREFIX"*)
        die "StreetLegal_Redline.exe hash has the expected prefix but differs in full: $exe_hash";;
    *)
        die "StreetLegal_Redline.exe hash mismatch: $exe_hash (expected prefix $EXPECTED_EXE_PREFIX)";;
esac
[ "$bak_hash" = "$EXPECTED_BAK" ] \
    || die "StreetLegal_Redline.bak.exe hash mismatch: $bak_hash (expected $EXPECTED_BAK)"

# --- Checksum manifest for the game's startup check (M7-04). Hashes are taken
# from the tree verified above and stay inside the git-ignored target. ---
MANIFEST="$TARGET/.slr-manifest"
(
    cd "$TARGET"
    find . -type f -print0 | xargs -0 shasum -a 256
) | awk -F'\t' -v OFS='\t' '
    FILENAME ~ /mapping\.tsv$/ { size[$2] = $3; next }
    {
        # shasum output: "<64 hex>  ./<path>"
        hash = substr($0, 1, 64); rel = substr($0, 69)
        if (!(rel in size)) { printf "NOT_IN_MAPPING\t%s\n", rel > "/dev/stderr"; failed = 1; next }
        print hash, size[rel], rel
    }
    END { if (failed) exit 1 }
' "$TDIR/mapping.tsv" - | LC_ALL=C sort -t "$TAB" -k3,3 > "$TDIR/manifest.rows" \
    || die "could not build the checksum manifest"
rows="$(wc -l < "$TDIR/manifest.rows" | tr -d ' ')"
[ "$rows" -eq "$EXPECTED_COUNT" ] \
    || die "manifest has $rows rows, expected exactly $EXPECTED_COUNT"
{ echo "# slr-stock-manifest v1"; cat "$TDIR/manifest.rows"; } > "$MANIFEST"
log "manifest: $MANIFEST ($rows files; SHA-256, size, path)"

log "OK: $actual files verified (count, sizes, msiextract tree, exe hashes, manifest)"
