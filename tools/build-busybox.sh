#!/bin/sh

# Builds the static busybox that sharun embeds to run AppRun.sh (src/builtin_shell.rs).
# It only has the ash shell, no applets. The bash extensions are off, except for
# ${var/pat/repl}, ${var//pat/repl} and ${var:offset:length}.
#
#   usage: build-busybox.sh <zig> <zig target> <output>
#   example: build-busybox.sh zig x86_64-linux-musl shell/busybox
#
# Needs make, a host cc, curl, tar, bzip2 and sha256sum.

set -eu

BUSYBOX_VERSION=1.37.0
BUSYBOX_SHA256=3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4

ZIG=$1
TARGET=$2
OUT=$(realpath -m "$3")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

curl -fL --retry 3 -o "$work/busybox.tar.bz2" \
	"https://busybox.net/downloads/busybox-$BUSYBOX_VERSION.tar.bz2"
echo "$BUSYBOX_SHA256  $work/busybox.tar.bz2" | sha256sum -c -
tar -xjf "$work/busybox.tar.bz2" -C "$work"
cd "$work/busybox-$BUSYBOX_VERSION"

# zig's linker rejects the map, verbose and sort flags that busybox always passes
sed -i \
	-e 's/-Wl,--warn-common//' \
	-e 's/-Wl,-Map,[^ ]*//' \
	-e 's/-Wl,--verbose//' \
	-e 's/-Wl,--sort-common//' \
	-e 's/-Wl,--sort-section,alignment//' \
	scripts/trylink

# ash controls all of its bash extensions with ASH_BASH_COMPAT, which stays off.
# These two have their own macro, point them back on so the parameter expansions
# keep working. The check makes a busybox that changed these lines fail the build
sed -i \
	-e 's/^#define    BASH_PATTERN_SUBST   ENABLE_ASH_BASH_COMPAT$/#define    BASH_PATTERN_SUBST   1/' \
	-e 's/^#define IF_BASH_PATTERN_SUBST       IF_ASH_BASH_COMPAT$/#define IF_BASH_PATTERN_SUBST(...) __VA_ARGS__/' \
	-e 's/^#define    BASH_SUBSTR          ENABLE_ASH_BASH_COMPAT$/#define    BASH_SUBSTR          1/' \
	-e 's/^#define IF_BASH_SUBSTR              IF_ASH_BASH_COMPAT$/#define IF_BASH_SUBSTR(...) __VA_ARGS__/' \
	shell/ash.c
[ "$(grep -cE '^#define +(IF_)?BASH_(PATTERN_SUBST|SUBSTR)' shell/ash.c)" = 4 ] &&
	[ "$(grep -cE '^#define +(IF_)?BASH_(PATTERN_SUBST|SUBSTR).*ASH_BASH_COMPAT' shell/ash.c)" = 0 ] ||
	{ echo "shell/ash.c does not look like busybox $BUSYBOX_VERSION, cannot keep \${var//a/b}" >&2; exit 1; }

# dash echo always interprets backslash escapes (\t, \n, \c, ...) and busybox
# only does after -e. Make it the default so scripts written for dash keep working
sed -i 's/^\tchar eflag = 0;$/\tchar eflag = '"'"'\\\\'"'"';/' coreutils/echo.c
[ "$(grep -cF "char eflag = '\\\\';" coreutils/echo.c)" = 2 ] ||
	{ echo "coreutils/echo.c does not look like busybox $BUSYBOX_VERSION, cannot make echo process escapes" >&2; exit 1; }

make allnoconfig >/dev/null

# allnoconfig writes "# CONFIG_X is not set" for every option, turn them on in place
enable() {
	if grep -q "^# CONFIG_$1 is not set" .config; then
		sed -i "s/^# CONFIG_$1 is not set/CONFIG_$1=y/" .config
	elif ! grep -q "^CONFIG_$1=" .config; then
		echo "CONFIG_$1=y" >> .config
	fi
}
# FEATURE_FANCY_ECHO: -n/-e, ASH_JOB_CONTROL: the kill, jobs, fg and bg builtins of dash
for o in STATIC LFS ASH SH_IS_ASH FEATURE_FANCY_ECHO ASH_JOB_CONTROL ASH_OPTIMIZE_FOR_SIZE ASH_INTERNAL_GLOB \
	ASH_ALIAS ASH_ECHO ASH_PRINTF ASH_TEST ASH_GETOPTS \
	ASH_CMDCMD ASH_EXPAND_PRMT ASH_RANDOM_SUPPORT FEATURE_SH_MATH \
	FEATURE_SH_MATH_64 FEATURE_SH_READ_FRAC FEATURE_SH_EXTRA_QUIET; do
	enable "$o"
done
yes "" | make oldconfig >/dev/null

make -j"$(nproc)" HOSTCC=cc CC="$ZIG cc -target $TARGET" \
	CFLAGS="-Os -flto -fno-unwind-tables -fno-asynchronous-unwind-tables -ffunction-sections -fdata-sections" \
	EXTRA_LDFLAGS="-flto -Wl,--gc-sections -Wl,--strip-all" SKIP_STRIP=y busybox >build.log 2>&1 || {
	tail -30 build.log >&2
	exit 1
}

mkdir -p "$(dirname "$OUT")"
cp busybox "$OUT"
ls -l "$OUT"
