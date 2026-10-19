#!/bin/sh

# Builds the static dash that sharun embeds to run AppRun.sh (src/builtin_shell.rs).
#
#   usage: build-dash.sh <zig> <zig target> <output>
#   example: build-dash.sh zig x86_64-linux-musl shell/dash
#
# Needs make, curl, tar, gzip, sha512sum and a build host that runs x86_64
# Linux binaries: dash generates its tables with helper programs that run
# during the build, those are always built for x86_64 musl.

set -eu

DASH_VERSION=0.5.13.5
# sha512 of the tarball as published next to it on
# http://gondor.apana.org.au/~herbert/dash/files/
DASH_SHA512=ae326c110713a9da6b7afb977ca6fd931793b03011f0a7aa2c42e873f116ed09448deea9ef5ca4515fe3afe24e210e0aaa580f7de89934edda9cf31ee44c94dd

ZIG=$1
TARGET=$2
OUT=$(realpath -m "$3")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

curl -fL --retry 3 -o "$work/dash.tar.gz" \
	"http://gondor.apana.org.au/~herbert/dash/files/dash-$DASH_VERSION.tar.gz"
echo "$DASH_SHA512  $work/dash.tar.gz" | sha512sum -c -
tar --no-same-owner -xzf "$work/dash.tar.gz" -C "$work"
cd "$work/dash-$DASH_VERSION"

# The signal name table comes from a helper that is built and run on the build
# host, so it has to see the signals of the libc the shell runs with. With the
# host gcc (glibc) SIGRTMIN is 34, musl has 35, and every RT signal name would
# be off by one. The signal numbers up to 31 are the same on all the targets.
HELPER_CC="$ZIG cc -target x86_64-linux-musl"

CFLAGS="-Os -flto -fno-unwind-tables -fno-asynchronous-unwind-tables -ffunction-sections -fdata-sections" \
LDFLAGS="-flto -static -Wl,--gc-sections -Wl,--strip-all" \
CC="$ZIG cc -target $TARGET" CC_FOR_BUILD="$HELPER_CC" \
	./configure --host="$TARGET" --enable-static >configure.log 2>&1 || {
	tail -30 configure.log >&2
	exit 1
}
make -j"$(nproc)" >make.log 2>&1 || {
	tail -30 make.log >&2
	exit 1
}

mkdir -p "$(dirname "$OUT")"
cp src/dash "$OUT"
ls -l "$OUT"
