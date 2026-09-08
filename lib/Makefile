# LD_PRELOAD helper libraries used by quick-sharun
#
# Override CC to cross-compile against an older glibc floor, example:
#   make CC="zig cc -target x86_64-linux-gnu.2.31" LDFLAGS=-s

CC      ?= cc
CFLAGS  ?= -O2 -Wall -Wextra
SOFLAGS  = -shared -fPIC
LDFLAGS ?=
LDLIBS   = -ldl

LIBS = anylinux.so gtk-class-fix.so

all: $(LIBS)

anylinux.so: anylinux.c
	$(CC) $(SOFLAGS) $(CFLAGS) $(LDFLAGS) $< -o $@ $(LDLIBS)

# fix-gnome-glycin.c is linked into gtk-class-fix.so, it is NOT built as
# its own library. Everything glycin related is reached via dlsym, the lib
# must never hold link-time references to glycin (see Pinta-AppImage#17)
gtk-class-fix.so: gtk-class-fix.c fix-gnome-glycin.c
	$(CC) $(SOFLAGS) $(CFLAGS) $(LDFLAGS) $^ -o $@ $(LDLIBS)

clean:
	rm -f $(LIBS)

.PHONY: all clean
