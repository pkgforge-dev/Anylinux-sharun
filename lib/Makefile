# LD_PRELOAD helper libraries used by quick-sharun
#
# Override CC to cross-compile against an older glibc floor, example:
#   make CC="zig cc -target x86_64-linux-gnu.2.31" LDFLAGS=-s

CC      ?= cc
CFLAGS  ?= -O2 -Wall -Wextra
SOFLAGS  = -shared -fPIC
LDFLAGS ?=
LDLIBS   = -ldl

LIBS = anylinux.so gtk-fix-nonsense.so glycin-fix.so

all: $(LIBS)

%.so: %.c
	$(CC) $(SOFLAGS) $(CFLAGS) $(LDFLAGS) $< -o $@ $(LDLIBS)

clean:
	rm -f $(LIBS)

.PHONY: all clean
