# LD_PRELOAD helper libraries used by quick-sharun
#
# Override CC to cross-compile against an older glibc floor, example:
#   make CC="zig cc -target x86_64-linux-gnu.2.31" LDFLAGS=-s

CC      ?= cc
CFLAGS  ?= -O2 -Wall -Wextra
SOFLAGS  = -shared -fPIC
LDFLAGS ?=
LDLIBS   = -ldl

LIBS = anylinux.so gtk-fix-nonsense.so glycin-fix.so path-mapping.so

all: $(LIBS)

# path-mapping.c includes its common header
path-mapping.so: path-mapping.c pathmap_common.h
# statmount needs a modern glibc/kernel header pair to compile and a
# runtime libc that wraps the syscall, with the 2.31 floor the wrapper
# could crash calling a null dlsym pointer, e.g. glibc < 2.39
path-mapping.so: CFLAGS += -DDISABLE_STATMOUNT

%.so: %.c
	$(CC) $(SOFLAGS) $(CFLAGS) $(LDFLAGS) $< -o $@ $(LDLIBS)

# ppc64 (BE) only: zig records the ELFv1 versions (GLIBC_2.3/2.4) that
# archlinuxpower's ELFv2 glibc does not define, so bump them to GLIBC_2.17.
# FIX_VERSIONS points at the `fix-versions` cargo bin (see tools/fix-versions.rs)
# https://github.com/pkgforge-dev/Anylinux-sharun/issues/11
FIX_VERSIONS ?= fix-versions
fix-versions: $(LIBS)
	$(FIX_VERSIONS) $(LIBS)

clean:
	rm -f $(LIBS)

.PHONY: all fix-versions clean
