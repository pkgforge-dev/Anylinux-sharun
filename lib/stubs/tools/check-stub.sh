#!/bin/sh
# Verify that a built stub exports exactly the symbols in its manifest.
set -eu

[ "$#" -eq 2 ] || {
	echo "usage: $0 STUB.SO SYMBOLS.TSV" >&2
	exit 1
}
lib=$1 manifest=$2

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

LC_ALL=C readelf --dyn-syms -W "$lib" | awk '
	$1 == "Num:" || NF < 8 { next }
	$4 ~ /^(FUNC|IFUNC|OBJECT)$/ && $(NF-1) != "UND" && $(NF-1) != "ABS" && index($NF, "@@") > 0 { print $NF }
' | LC_ALL=C sort -u > "$tmp/actual"

awk -F'\t' '!/^#/ && NF { print $1 "@@" $2 }' "$manifest" |
LC_ALL=C sort -u > "$tmp/expected"

missing=$(LC_ALL=C comm -23 "$tmp/expected" "$tmp/actual")
extra=$(LC_ALL=C comm -13 "$tmp/expected" "$tmp/actual")

if [ -n "$missing" ] || [ -n "$extra" ]; then
	if [ -n "$missing" ]; then
		echo "missing exports:" >&2
		printf '%s\n' "$missing" | sed 's/^/  /' >&2
	fi
	if [ -n "$extra" ]; then
		echo "unexpected exports:" >&2
		printf '%s\n' "$extra" | sed 's/^/  /' >&2
	fi
	exit 1
fi

echo "$lib: $(wc -l < "$tmp/actual") symbols OK"
