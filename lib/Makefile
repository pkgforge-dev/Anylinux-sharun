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

# ppc64 (BE) only: zig's powerpc64-linux-gnu stub is the ELFv1 one
# (GLIBC_2.3/2.4) which archlinuxpower's glibc does not define. Link against
# a BE build of zig's ppc64le ELFv2 stub instead, see tools/gen-elfv2-stubs.sh
# https://github.com/pkgforge-dev/Anylinux-sharun/issues/11
PPC64_ELFV2_STUB ?=

ifneq ($(PPC64_ELFV2_STUB),)
PPC64_ELFV2_STUBS = $(PPC64_ELFV2_STUB)/libc.so.6 $(wildcard $(PPC64_ELFV2_STUB)/libdl.so.2)

%.so: %.c
	$(CC) $(CFLAGS) -fPIC -c $< -o $*.o
	$(CC) -shared -nostdlib $(LDFLAGS) $*.o $(PPC64_ELFV2_STUBS) -o $@
	rm -f $*.o
else
%.so: %.c
	$(CC) $(SOFLAGS) $(CFLAGS) $(LDFLAGS) $< -o $@ $(LDLIBS)
endif

clean:
	rm -f $(LIBS) *.o

.PHONY: all clean
