#!/bin/sh

# Tests the part of `sharun -g` that removes absolute rpaths and absolute
# needed library paths from the ELF files of an AppDir.
# Needs a C compiler and readelf, builds the test files for the host.
#
#   usage: test-rpath.sh <sharun> [cc]

set -u

sharun=$1
cc=${2:-cc}
case $sharun in
/*) ;;
*) sharun=$PWD/$sharun ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
cd "$tmp" || exit 1

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

# expect <name> <readelf -d regex> <file>: the dynamic section has a match
dyn() { readelf -d "$2" 2>/dev/null | grep -Eq "$1"; }

echo 'int b(void){return 7;}' > b.c
echo 'int b(void); int a(void){return b()+1;}' > a.c
echo 'int a(void); int main(void){return a()==8?0:1;}' > m.c

build() { # <dtags flag> <dir>
	d=$2
	mkdir -p "$d/lib/sub" "$d/lib/a/b/c/d/e/f/g" "$d/usr/lib" "$d/shared/bin"
	# without a soname the linker records the path it was given as the
	# needed entry, which is how a library gets an absolute path in it
	"$cc" -shared -fPIC b.c -o "$d/lib/libB.so" &&
	"$cc" -shared -fPIC a.c -o "$d/lib/libA.so.1" \
		-Wl,-rpath,/usr/lib "$1" "$PWD/$d/lib/libB.so" &&
	"$cc" -shared -fPIC b.c -o "$d/lib/sub/libC.so" -Wl,-rpath,'$ORIGIN:/usr/lib' "$1" &&
	"$cc" -shared -fPIC b.c -o "$d/lib/libD.so" -Wl,-rpath,'/opt/x:$ORIGIN' "$1" &&
	"$cc" -shared -fPIC b.c -o "$d/lib/libE.so" &&
	"$cc" -shared -fPIC b.c -o "$d/lib/a/b/c/d/e/f/g/libdeep.so" -Wl,-rpath,/deep "$1" &&
	"$cc" -shared -fPIC b.c -o "$d/usr/lib/libusr.so.2" -Wl,-rpath,/usr/lib "$1" &&
	"$cc" m.c -o "$d/shared/bin/app" -Wl,-rpath,/opt/host/lib "$1" \
		"$PWD/$d/lib/libA.so.1" &&
	"$cc" -static m.c a.c b.c -o "$d/shared/bin/static_bin" &&
	echo 'not an elf file' > "$d/lib/fake.so" &&
	"$cc" -shared -fPIC b.c -o "$d/lib/libRO.so" -Wl,-rpath,/ro "$1" &&
	chmod 555 "$d/lib/libRO.so" &&
	ln -s libD.so "$d/lib/libDlink.so" &&
	cp "$sharun" "$d/sharun"
}

for dtags in --disable-new-dtags --enable-new-dtags; do
	echo "# $dtags"
	rm -rf app
	if ! build "-Wl,$dtags" app; then
		echo "cannot build the test files" >&2
		exit 1
	fi
	( cd app && sha256sum lib/libE.so lib/fake.so shared/bin/static_bin lib/sub/libC.so lib/libD.so > ../before.sum )
	dyn 'RPATH|RUNPATH' app/lib/libA.so.1 && dyn 'NEEDED.*\[/' app/lib/libA.so.1 &&
		dyn 'NEEDED.*\[/' app/shared/bin/app ||
		{ fail "the test files have what the test is about"; continue; }
	link_before=$(readlink app/lib/libDlink.so)
	inode_d=$(ls -i app/lib/libD.so | cut -d' ' -f1)

	out=$(cd app && ./sharun -g 2>&1)
	rc=$?
	if [ "$rc" = 0 ]; then ok "sharun -g exits 0"; else fail "sharun -g exits 0" "rc=$rc" "$out"; fi

	for f in lib/libA.so.1 lib/libD.so lib/a/b/c/d/e/f/g/libdeep.so usr/lib/libusr.so.2 shared/bin/app lib/libRO.so; do
		if dyn 'RPATH|RUNPATH' "app/$f"; then fail "absolute rpath removed from $f" "$(readelf -d "app/$f" | grep -E 'RPATH|RUNPATH')"; else ok "absolute rpath removed from $f"; fi
	done

	if dyn 'NEEDED.*\[/' app/lib/libA.so.1 || dyn 'NEEDED.*\[/' app/shared/bin/app; then
		fail "absolute needed paths became basenames" "$(readelf -d app/lib/libA.so.1 app/shared/bin/app | grep NEEDED)"
	elif dyn 'NEEDED.*\[libB.so\]' app/lib/libA.so.1 && dyn 'NEEDED.*\[libA.so.1\]' app/shared/bin/app; then
		ok "absolute needed paths became basenames"
	else
		fail "absolute needed paths became basenames" "$(readelf -d app/lib/libA.so.1 app/shared/bin/app | grep NEEDED)"
	fi

	if dyn 'RPATH|RUNPATH.*\$ORIGIN:/usr/lib' app/lib/sub/libC.so; then ok "an rpath that does not start with / is kept"; else fail "an rpath that does not start with / is kept"; fi

	( cd app && sha256sum lib/libE.so lib/fake.so shared/bin/static_bin lib/sub/libC.so > ../after.sum )
	grep -v 'libD.so' before.sum | cmp -s - after.sum && ok "files with nothing to fix are byte for byte the same" || fail "files with nothing to fix are byte for byte the same"

	if [ "$(readlink app/lib/libDlink.so)" = "$link_before" ] && [ -L app/lib/libDlink.so ]; then ok "symlinks stay symlinks"; else fail "symlinks stay symlinks"; fi

	mode=$(stat -c %a app/lib/libRO.so)
	if [ "$mode" = 555 ]; then ok "the mode of a read-only file is kept"; else fail "the mode of a read-only file is kept" "mode=$mode"; fi

	if [ -z "$(find app -name '*.sharun-rpath.*')" ]; then ok "no temporary files are left"; else fail "no temporary files are left" "$(find app -name '*.sharun-rpath.*')"; fi

	( cd app && LD_LIBRARY_PATH=$PWD/lib ./shared/bin/app )
	rc=$?
	if [ "$rc" = 0 ]; then ok "the binary still runs and finds its library by name"; else fail "the binary still runs and finds its library by name" "rc=$rc"; fi

	cp -a app/lib/libA.so.1 libA.once
	out=$(cd app && ./sharun -g 2>&1)
	if cmp -s libA.once app/lib/libA.so.1 && ! printf '%s' "$out" | grep -q 'removed full'; then ok "a second run changes nothing"; else fail "a second run changes nothing" "$out"; fi
done

echo
echo "$((total - failed)) of $total passed"
[ "$failed" = 0 ]
