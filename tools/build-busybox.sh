#!/bin/sh

# Builds the static busybox that sharun embeds to run AppRun.sh (src/builtin_shell.rs).
# It only has the ash shell, no applets.
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

make allnoconfig >/dev/null

# allnoconfig writes "# CONFIG_X is not set" for every option, turn them on in place
enable() {
	if grep -q "^# CONFIG_$1 is not set" .config; then
		sed -i "s/^# CONFIG_$1 is not set/CONFIG_$1=y/" .config
	elif ! grep -q "^CONFIG_$1=" .config; then
		echo "CONFIG_$1=y" >> .config
	fi
}
sed -i 's/^CONFIG_SH_IS_ASH=.*/# CONFIG_SH_IS_ASH is not set/' .config
for o in STATIC LFS ASH SH_IS_ASH ASH_OPTIMIZE_FOR_SIZE ASH_INTERNAL_GLOB \
	ASH_BASH_COMPAT ASH_ALIAS ASH_ECHO ASH_PRINTF ASH_TEST ASH_GETOPTS \
	ASH_CMDCMD ASH_EXPAND_PRMT ASH_RANDOM_SUPPORT FEATURE_SH_MATH \
	FEATURE_SH_MATH_64 FEATURE_SH_READ_FRAC FEATURE_SH_EXTRA_QUIET; do
	enable "$o"
done
yes "" | make oldconfig >/dev/null

make -j"$(nproc)" HOSTCC=cc CC="$ZIG cc -target $TARGET" \
	CFLAGS="-Os -ffunction-sections -fdata-sections" \
	EXTRA_LDFLAGS="-Wl,--gc-sections -Wl,--strip-all" SKIP_STRIP=y busybox >build.log 2>&1 || {
	tail -30 build.log >&2
	exit 1
}

mkdir -p "$(dirname "$OUT")"
cp busybox "$OUT"
ls -l "$OUT"
