#!/bin/sh

# Checks how sharun picks the real program for a bin/<name> hardlink:
#   1. <AppDir>/.bin/<name>
#   2. <AppDir>/shared/bin/<name> when there is no .bin/<name>
# and that a program in .bin keeps its name in /proc/self/exe. An AppDir with
# only a .bin directory, without shared, has to work too.
#
# It uses /usr/bin/echo as the program that is found and /usr/bin/true as the
# one that must lose, so what is printed says which one ran. Needs a dynamic
# echo and true on the host.
#
#   usage: test-dot-bin.sh <sharun>

set -u

sharun=$1
case $sharun in
/*) ;;
*) sharun=$PWD/$sharun ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
app=$tmp/AppDir
mkdir -p "$app/bin" "$app/lib"

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
	# an empty PATH stops sharun from falling back to a program of the host
	got=$(env -i PATH=/nonexistent "$app/bin/prog" dot-bin-ran 2>"$tmp/err")
	rc=$?
	if [ "$got" != "$2" ] || [ "$rc" != "$3" ]; then
		failed=$((failed + 1))
		printf 'FAIL: %s\n  want: %s (rc %s)\n  got:  %s (rc %s)\n  err:  %s\n' \
			"$1" "$2" "$3" "$got" "$rc" "$(cat "$tmp/err")"
	else
		printf 'ok:   %s\n' "$1"
	fi
}

mkdir -p "$app/shared/bin"
cp "$echo_bin" "$app/shared/bin/prog"
check 'shared/bin/<name> when there is no .bin' dot-bin-ran 0

mkdir -p "$app/.bin"
cp "$true_bin" "$app/.bin/prog"
check '.bin/<name> wins over shared/bin' '' 0

rm -rf "$app/shared"
cp "$echo_bin" "$app/.bin/prog"
check 'AppDir with .bin and no shared' dot-bin-ran 0

rm "$app/.bin/prog"
check 'no program anywhere fails' '' 1

# static program: executed by the kernel, /proc/self/exe keeps the name.
# Needs a C compiler that can link statically.
cat > "$tmp/exe.c" <<'C'
#include <stdio.h>
#include <unistd.h>
int main(void) {
	char b[4096];
	ssize_t n = readlink("/proc/self/exe", b, sizeof(b) - 1);
	if (n < 0) return 1;
	b[n] = 0;
	puts(b);
	return 0;
}
C
if ${CC:-cc} -static -o "$tmp/exe-static" "$tmp/exe.c" 2>/dev/null; then
	cp "$tmp/exe-static" "$app/.bin/prog"
	got=$(env -i PATH=/nonexistent "$app/bin/prog" 2>"$tmp/err")
	total=$((total + 1))
	if [ "$got" = "$app/.bin/prog" ]; then
		printf 'ok:   static .bin/<name> keeps its name in /proc/self/exe\n'
	else
		failed=$((failed + 1))
		printf 'FAIL: static .bin/<name>\n  want: %s\n  got:  %s\n  err:  %s\n' \
			"$app/.bin/prog" "$got" "$(cat "$tmp/err")"
	fi
else
	echo 'SKIP: no static linking, static .bin/<name> not checked'
fi

printf '%s of %s checks failed\n' "$failed" "$total"
[ "$failed" = 0 ]
