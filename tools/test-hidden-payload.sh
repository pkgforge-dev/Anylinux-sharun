#!/bin/sh

# Checks how sharun picks the real program for a bin/<name> hardlink:
#   1. bin/.<name>.sharun, next to the hardlink, does not need the exec bit
#   2. shared/bin/<name>, when there is no hidden payload
#   3. the hidden payload wins when both exist
# It uses /usr/bin/echo as the payload and /usr/bin/true as the loser, so what
# is printed says which one ran. Needs a dynamic echo and true on the host.
#
#   usage: test-hidden-payload.sh <sharun>

set -u

sharun=$1
case $sharun in
/*) ;;
*) sharun=$PWD/$sharun ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
app=$tmp/AppDir
mkdir -p "$app/bin" "$app/shared/bin" "$app/lib"

echo_bin=$(command -v echo)
[ "$echo_bin" = echo ] && echo_bin=/usr/bin/echo
true_bin=/usr/bin/true
for b in "$echo_bin" "$true_bin"; do
	[ -x "$b" ] || { echo "SKIP: $b not found"; exit 0; }
done

# the loader and the libraries of both programs
for b in "$echo_bin" "$true_bin"; do
	for l in $(ldd "$b" | awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^\//) print $i }'); do
		cp -L "$l" "$app/lib/" 2>/dev/null
	done
done

cp "$sharun" "$app/sharun"
ln "$app/sharun" "$app/bin/prog"

total=0
failed=0

# check <description> <expected stdout> <expected exit code>
check() {
	total=$((total + 1))
	got=$("$app/bin/prog" hidden-payload-ran 2>"$tmp/err")
	rc=$?
	if [ "$got" != "$2" ] || [ "$rc" != "$3" ]; then
		failed=$((failed + 1))
		printf 'FAIL: %s\n  want: %s (rc %s)\n  got:  %s (rc %s)\n  err:  %s\n' \
			"$1" "$2" "$3" "$got" "$rc" "$(cat "$tmp/err")"
	else
		printf 'ok:   %s\n' "$1"
	fi
}

cp "$echo_bin" "$app/shared/bin/prog"
check 'classic shared/bin/<name>' hidden-payload-ran 0

cp "$true_bin" "$app/bin/.prog.sharun"
chmod -x "$app/bin/.prog.sharun"
check 'hidden payload wins over shared/bin' '' 0

rm "$app/shared/bin/prog"
cp "$echo_bin" "$app/bin/.prog.sharun"
chmod -x "$app/bin/.prog.sharun"
check 'hidden payload without exec bit, no shared/bin' hidden-payload-ran 0

rm "$app/bin/.prog.sharun"
check 'no payload anywhere fails' '' 1

printf '%s of %s checks failed\n' "$failed" "$total"
[ "$failed" = 0 ]
