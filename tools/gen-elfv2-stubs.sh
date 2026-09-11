#!/bin/sh
# Build big-endian ELFv2 link stubs from zig's own ppc64le glibc stubs.
#
# ppc64 (BE) is ELFv2 but zig's `powerpc64-linux-gnu` abilist is the ELFv1 one
# (GLIBC_2.3/2.4), which archlinuxpower's glibc does not define.  The ppc64le
# abilist is the same ELFv2 version table, so assemble its stubs for BE and
# link the ppc64 preloads against those.  The stubs are link-time only and are
# never shipped.  No download: they already ship inside zig.
#
# https://github.com/pkgforge-dev/Anylinux-sharun/issues/11
#
# usage: gen-elfv2-stubs.sh <zig> <glibc-version> <outdir>
set -eu
ZIG=$1
GLIBC=$2
OUT=$3

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf 'void _z(void){}\n' > "$tmp/z.c"
"$ZIG" cc -target "powerpc64le-linux-gnu.$GLIBC" -shared -fPIC "$tmp/z.c" -o "$tmp/z.so"

src=
for d in $(find "${ZIG_GLOBAL_CACHE_DIR:-$HOME/.cache/zig}" -name all.map -printf '%h\n' 2>/dev/null); do
	[ -f "$d/libc.so.6" ] || continue
	readelf -h "$d/libc.so.6" 2>/dev/null | grep -q 'little endian' || continue
	readelf -h "$d/libc.so.6" 2>/dev/null | grep -q 'PowerPC64' || continue
	src=$d
	break
done
[ -n "$src" ] || { echo "gen-elfv2-stubs: no ppc64le stub in the zig cache" >&2; exit 1; }

mkdir -p "$OUT"
for pair in c:libc.so.6 dl:libdl.so.2; do
	s=${pair%%:*}
	soname=${pair#*:}
	[ -f "$src/$s.s" ] || continue
	"$ZIG" cc -target "powerpc64-linux-gnu.$GLIBC" -shared -nostdlib \
		-Wl,-z,notext -Wl,-soname,"$soname" \
		-Wl,--version-script="$src/all.map" \
		"$src/$s.s" -o "$OUT/$soname"
	echo "gen-elfv2-stubs: $OUT/$soname"
done
