# Dummy replacements for libsystemd and libsndfile.
#
# The stubs have no DT_NEEDED entries and no GLIBC_* version requirements,
# so they can be built for every supported arch with:
#
#   make CC="zig cc -target aarch64-linux-gnu.2.31" LDFLAGS=-s
#
# See README.md for what each stub implements.

CC      ?= cc
CFLAGS  ?= -O2 -Wall -Wextra
LDFLAGS ?=

OUT   := out
GEN   := $(OUT)/gen
STUBLDFLAGS := -nostdlib -Wl,-z,defs

STUBS := $(OUT)/libsystemd.so.0 $(OUT)/libsndfile.so.1

all: $(STUBS)

manifests:
	sh tools/generate-manifests.sh

$(OUT) $(GEN):
	mkdir -p $@

$(GEN)/systemd.c $(GEN)/systemd.map: systemd/symbols.tsv systemd/custom.txt tools/gen-stub.sh | $(GEN)
	sh tools/gen-stub.sh \
		--manifest systemd/symbols.tsv \
		--custom systemd/custom.txt \
		--out-c $(GEN)/systemd.c \
		--out-map $(GEN)/systemd.map

$(GEN)/sndfile.c $(GEN)/sndfile.map: sndfile/symbols.tsv sndfile/custom.txt tools/gen-stub.sh | $(GEN)
	sh tools/gen-stub.sh \
		--manifest sndfile/symbols.tsv \
		--custom sndfile/custom.txt \
		--out-c $(GEN)/sndfile.c \
		--out-map $(GEN)/sndfile.map

# Compile separately from linking: -nostdlib disables zig cc's header
# search paths too, so it must only appear in the link command.
$(GEN)/%.o: $(GEN)/%.c
	$(CC) $(CFLAGS) -fPIC -c $< -o $@

$(GEN)/overrides-systemd.o: systemd/overrides.c | $(GEN)
	$(CC) $(CFLAGS) -fPIC -c $< -o $@

$(GEN)/overrides-sndfile.o: sndfile/overrides.c | $(GEN)
	$(CC) $(CFLAGS) -fPIC -c $< -o $@

$(OUT)/libsystemd.so.0: $(GEN)/systemd.c $(GEN)/systemd.map $(GEN)/systemd.o $(GEN)/overrides-systemd.o | $(OUT)
	$(CC) -shared $(LDFLAGS) $(STUBLDFLAGS) \
		-Wl,-soname,libsystemd.so.0 \
		-Wl,--version-script=$(GEN)/systemd.map \
		-o $@ $(GEN)/systemd.o $(GEN)/overrides-systemd.o

$(OUT)/libsndfile.so.1: $(GEN)/sndfile.c $(GEN)/sndfile.map $(GEN)/sndfile.o $(GEN)/overrides-sndfile.o | $(OUT)
	$(CC) -shared $(LDFLAGS) $(STUBLDFLAGS) \
		-Wl,-soname,libsndfile.so.1 \
		-Wl,--version-script=$(GEN)/sndfile.map \
		-o $@ $(GEN)/sndfile.o $(GEN)/overrides-sndfile.o

check:
	@test -f $(OUT)/libsystemd.so.0 || { echo "error: build the stubs first" >&2; exit 1; }
	@test -f $(OUT)/libsndfile.so.1 || { echo "error: build the stubs first" >&2; exit 1; }
	sh tools/check-stub.sh $(OUT)/libsystemd.so.0 systemd/symbols.tsv
	sh tools/check-stub.sh $(OUT)/libsndfile.so.1 sndfile/symbols.tsv

clean:
	rm -rf $(OUT)

.PHONY: all check clean manifests
