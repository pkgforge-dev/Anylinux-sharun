#!/bin/sh
# Regenerate both ABI manifests from the real libraries.
#
# This is the only step that needs systemd, libsndfile and their headers.
# It is run by the release CI inside an Arch container, so the manifests do
# not have to be committed. It can also be run manually on an Arch host:
#
#     make -C lib/stubs manifests
#
# or with explicit paths:
#
#     SYSTEMD_LIB=... SNDFILE_LIB=... \
#     SYSTEMD_INCLUDE_DIR=... SNDFILE_INCLUDE_DIR=... \
#     sh lib/stubs/tools/generate-manifests.sh
set -eu

root=$(CDPATH= cd -- "${0%/*}/../../.." && pwd)
cd "$root"

systemd_lib=${SYSTEMD_LIB:-/usr/lib/libsystemd.so.0}
sndfile_lib=${SNDFILE_LIB:-/usr/lib/libsndfile.so.1}
systemd_include=${SYSTEMD_INCLUDE_DIR:-${INCLUDE_DIR:-/usr/include}}
sndfile_include=${SNDFILE_INCLUDE_DIR:-${INCLUDE_DIR:-/usr/include}}

[ -e "$systemd_lib" ] || {
	echo "error: $systemd_lib not found (install systemd)" >&2
	exit 1
}
[ -e "$sndfile_lib" ] || {
	echo "error: $sndfile_lib not found (install libsndfile)" >&2
	exit 1
}
[ -d "$systemd_include/systemd" ] || {
	echo "error: $systemd_include/systemd not found (install the systemd headers)" >&2
	exit 1
}
[ -f "$sndfile_include/sndfile.h" ] || {
	echo "error: $sndfile_include/sndfile.h not found (install the libsndfile headers)" >&2
	exit 1
}

pkg_version() {
	pkg=$(pacman -Qoq "$(readlink -f "$1")" 2>/dev/null || :)
	[ -n "$pkg" ] || return 0
	pacman -Q "$pkg" 2>/dev/null | cut -d' ' -f2
}

systemd_ver=unknown
sndfile_ver=unknown
if command -v pacman >/dev/null 2>&1; then
	systemd_ver=$(pkg_version "$systemd_lib")
	sndfile_ver=$(pkg_version "$sndfile_lib")
fi
systemd_note="systemd-libs ${systemd_ver:-unknown} (Arch), $(readlink -f "$systemd_lib")"
sndfile_note="libsndfile ${sndfile_ver:-unknown} (Arch), $(readlink -f "$sndfile_lib")"

mkdir -p lib/stubs/systemd lib/stubs/sndfile

sh lib/stubs/tools/gen-manifest.sh \
	--lib "$systemd_lib" \
	--include-dir "$systemd_include" \
	--include systemd/sd-bus.h \
	--include systemd/sd-daemon.h \
	--include systemd/sd-device.h \
	--include systemd/sd-event.h \
	--include systemd/sd-gpt.h \
	--include systemd/sd-hwdb.h \
	--include systemd/sd-id128.h \
	--include systemd/sd-json.h \
	--include systemd/sd-journal.h \
	--include systemd/sd-login.h \
	--include systemd/sd-path.h \
	--include systemd/sd-varlink.h \
	--include systemd/sd-varlink-idl.h \
	--note "$systemd_note" \
	--out lib/stubs/systemd/symbols.tsv

sh lib/stubs/tools/gen-manifest.sh \
	--lib "$sndfile_lib" \
	--include-dir "$sndfile_include" \
	--include sndfile.h \
	--note "$sndfile_note" \
	--out lib/stubs/sndfile/symbols.tsv
