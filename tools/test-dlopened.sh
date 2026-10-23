#!/bin/sh

# Tests `sharun --get-dlopened` against a small program that dlopens, sleeps,
# forks and ignores signals as told (tools/dlopened-tests/probe.c).
# Needs a C compiler and a glibc system, LD_DEBUG=libs is a glibc feature.
#
#   usage: test-dlopened.sh <sharun> [cc]

set -u

sharun=$1
cc=${2:-cc}
case $sharun in
/*) ;;
*) sharun=$PWD/$sharun ;;
esac
dir=$(cd "$(dirname "$0")" && pwd)/dlopened-tests

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
cd "$tmp" || exit 1

mkdir lib
echo 'int t1(void){return 1;}' > t1.c
echo 'int t3(void){return 3;}' > t3.c
echo 'int t3(void); int t2(void){return t3();}' > t2.c
echo 'int main(void){return 0;}' > static.c
{
	"$cc" -shared -fPIC t1.c -o lib/libt1.so &&
	"$cc" -shared -fPIC t3.c -o lib/libt3.so &&
	"$cc" -shared -fPIC t2.c -o lib/libt2.so -Llib -lt3 -Wl,-rpath,"$tmp/lib" &&
	"$cc" "$dir/probe.c" -o probe -ldl &&
	"$cc" -static static.c -o static_bin
} || { echo "cannot build the test programs" >&2; exit 1; }

total=0
failed=0

ok() {
	total=$((total + 1))
	echo "ok   $1"
}

fail() {
	total=$((total + 1))
	failed=$((failed + 1))
	echo "FAIL $1"
	shift
	for line in "$@"; do echo "     $line"; done
}

now_ms() {
	date +%s%N | cut -b1-13
}

# run <name> <sharun args...>: output in $out, exit code in $rc, seconds*1000 in $ms
run() {
	start=$(now_ms)
	out=$("$sharun" --get-dlopened "$@" 2>"$tmp/stderr")
	rc=$?
	ms=$(( $(now_ms) - start ))
}

has() {
	printf '%s\n' "$out" | grep -q "$1"
}

# check <name> <condition...>
check() {
	name=$1
	shift
	if "$@"; then ok "$name"; else fail "$name" "rc=$rc ms=$ms" "output: $(printf '%s' "$out" | tr '\n' ' ')" "stderr: $(cat "$tmp/stderr")"; fi
}

lt() { [ "$ms" -lt "$1" ]; }
ge() { [ "$ms" -ge "$1" ]; }

alive() {
	# any process whose command line has the marker
	for f in /proc/[0-9]*/cmdline; do
		if tr '\0' ' ' < "$f" 2>/dev/null | grep -q "$1"; then return 0; fi
	done
	return 1
}

export LD_LIBRARY_PATH="$tmp/lib"

run ./probe open:libt1.so
if has 'libt1.so' && has 'libc.so'; then ok "lists the libraries of a program"; else fail "lists the libraries of a program" "output: $out"; fi

run ./probe open:libt2.so
if has 'libt2.so' && has 'libt3.so'; then ok "lists the dependencies of a dlopened library"; else fail "lists the dependencies of a dlopened library" "output: $out"; fi

run ./probe open:libt1.so
sorted=$(printf '%s\n' "$out" | sort -u)
if [ "$out" = "$sorted" ] && [ "$rc" = 0 ]; then ok "output is sorted, unique and the exit code is 0"; else fail "output is sorted, unique and the exit code is 0" "rc=$rc"; fi

# idle stop
run ./probe open:libt1.so sleep:700 open:libt2.so
if [ "$rc" = 0 ] && ! has libt2.so && [ "$ms" -lt 2500 ]; then ok "stops after the default idle time, a later dlopen is not counted"; else fail "stops after the default idle time, a later dlopen is not counted" "rc=$rc ms=$ms out=$out"; fi

run --idle-time 2000 ./probe open:libt1.so sleep:700 open:libt2.so
if has libt2.so; then ok "--idle-time makes it wait for a later dlopen"; else fail "--idle-time makes it wait for a later dlopen" "ms=$ms out=$out"; fi

run --idle-time 0 --max-time 3 ./probe open:libt1.so sleep:1200 open:libt2.so
if has libt2.so; then ok "--idle-time 0 turns the idle stop off"; else fail "--idle-time 0 turns the idle stop off" "ms=$ms out=$out"; fi

# max time
run --idle-time 0 --max-time 1 ./probe open:libt1.so hang
if [ "$ms" -ge 900 ] && [ "$ms" -lt 2600 ] && has libt1.so; then ok "--max-time ends a program that never stops"; else fail "--max-time ends a program that never stops" "ms=$ms out=$out"; fi

run --idle-time 0 --max-time 0.5 ./probe open:libt1.so hang
if [ "$ms" -ge 400 ] && [ "$ms" -lt 2000 ] && has libt1.so; then ok "--max-time takes fractions"; else fail "--max-time takes fractions" "ms=$ms"; fi

run --idle-time 400 ./probe open:libt1.so sleep:200 open:libt2.so
if has libt2.so; then ok "the idle time counts from the last new library"; else fail "the idle time counts from the last new library" "ms=$ms out=$out"; fi

# exits
run ./probe
if [ "$rc" = 0 ] && [ "$ms" -lt 1500 ]; then ok "a program that exits at once ends the trace at once"; else fail "a program that exits at once ends the trace at once" "rc=$rc ms=$ms"; fi

run --max-time 3 ./probe open:libt1.so 'argv:'"$tmp"'/argv.txt' one 'two words' --max-time=9 -x
if [ "$(cat "$tmp/argv.txt")" = "one
two words
--max-time=9
-x" ]; then ok "arguments after the binary go to the binary untouched"; else fail "arguments after the binary go to the binary untouched" "$(cat "$tmp/argv.txt" 2>&1)"; fi

run ./probe stdout:from-the-program open:libt1.so
if ! has from-the-program && has libt1.so; then ok "the program's stdout does not end up in the output"; else fail "the program's stdout does not end up in the output" "out=$out"; fi

run ./probe 'stderr:calling init: /fake/no-pid.so' 'stderr:app: calling init: /fake/not-a-lib.so' open:libt1.so
if ! has /fake/ && has libt1.so; then ok "lines without the pid and tab of LD_DEBUG are not taken for libraries"; else fail "lines without the pid and tab of LD_DEBUG are not taken for libraries" "out=$out"; fi

# process tracking
run --idle-time 1500 ./probe bg sleep:300 open:libt2.so
if has libt2.so && has libt3.so; then ok "a launcher that exits does not end the trace, its child is traced"; else fail "a launcher that exits does not end the trace, its child is traced" "ms=$ms out=$out"; fi

run --idle-time 0 --max-time 1 ./probe bg open:libt1.so hang marker-bg-1
sleep 1
if ! alive marker-bg-1; then ok "a child started by the program is killed when the trace ends"; else fail "a child started by the program is killed when the trace ends"; pkill -f marker-bg-1; fi

run --idle-time 0 --max-time 1 ./probe ignoreterm open:libt1.so hang marker-term-1
sleep 1
if ! alive marker-term-1 && [ "$ms" -lt 4000 ]; then ok "a program that ignores SIGTERM is killed"; else fail "a program that ignores SIGTERM is killed" "ms=$ms"; pkill -9 -f marker-term-1; fi

run --max-time 0.5 ./probe open:libt1.so bg ignoreterm hang marker-both-1
if ! alive marker-both-1; then ok "a child that ignores SIGTERM is killed too"; else fail "a child that ignores SIGTERM is killed too"; pkill -9 -f marker-both-1; fi

# the interrupt
"$sharun" --get-dlopened --idle-time 0 --max-time 30 ./probe open:libt1.so hang marker-int-1 >"$tmp/int.out" 2>&1 &
pid=$!
sleep 1
kill -TERM $pid
wait $pid
rc=$?
sleep 1.5
if [ "$rc" = 130 ] && ! alive marker-int-1; then ok "SIGTERM to sharun kills the program and exits 130"; else fail "SIGTERM to sharun kills the program and exits 130" "rc=$rc"; pkill -9 -f marker-int-1; fi

# environment
# not through run(): the tools it calls would write the loader log themselves
out=$(LD_DEBUG=files LD_DEBUG_OUTPUT="$tmp/ld.log" "$sharun" --get-dlopened ./probe open:libt1.so 2>/dev/null)
if has libt1.so && [ ! -e "$tmp/ld.log" ] && ! ls "$tmp"/ld.log.* >/dev/null 2>&1; then ok "LD_DEBUG and LD_DEBUG_OUTPUT of the caller are replaced"; else fail "LD_DEBUG and LD_DEBUG_OUTPUT of the caller are replaced" "out=$out"; fi

# the other kinds of binary
run ./static_bin
if [ "$rc" = 0 ] && [ -z "$out" ]; then ok "a static binary has no libraries and is not an error"; else fail "a static binary has no libraries and is not an error" "rc=$rc out=$out"; fi

run /nonexistent/binary
if [ "$rc" = 1 ] && grep -q 'Failed to execute /nonexistent/binary' "$tmp/stderr"; then ok "a missing binary is an error"; else fail "a missing binary is an error" "rc=$rc"; fi

run
if [ "$rc" = 1 ] && grep -q 'missing BINARY' "$tmp/stderr"; then ok "no binary is an error"; else fail "no binary is an error" "rc=$rc"; fi

run --max-time 0 ./probe
if [ "$rc" = 1 ] && grep -q 'invalid --max-time' "$tmp/stderr"; then ok "--max-time 0 is an error"; else fail "--max-time 0 is an error" "rc=$rc"; fi

# the same libraries as the LD_DEBUG=libs pipeline quick-sharun used
expected=$(LD_DEBUG=libs ./probe open:libt2.so open:libt1.so 2>&1 | awk '/calling init/{print $NF}' | sort -u)
run --idle-time 0 --max-time 2 ./probe open:libt2.so open:libt1.so
if [ "$out" = "$expected" ] && [ -n "$out" ]; then ok "same libraries as LD_DEBUG=libs piped through awk"; else fail "same libraries as LD_DEBUG=libs piped through awk" "expected: $expected" "got: $out"; fi

echo
echo "$((total - failed)) of $total passed"
[ "$failed" = 0 ]
