# Dummy libraries

Stub replacements for libraries that end up in almost every AppImage only
because something else links them transitively:

| Stub | Pulled in by | Real dependency chain |
|---|---|---|
| `libsystemd.so.0` | `libdbus-1.so.3`, `libpulsecommon` | `libpulse` → `libpulsecommon` → `libsystemd` |
| `libsndfile.so.1` | `libpulsecommon` | `libpulse` → `libpulsecommon` → `libsndfile` → `libopus` + FLAC + mpg123 + lame + vorbis + ogg |

`libpulsecommon` uses libsystemd only for `sd_journal_send()`, and libdbus
only for `sd_listen_fds()` / `sd_is_socket()`. Its use of libsndfile is
limited to the `pa_sndfile_*` helpers used by `pacat`/`paplay`/`pactl`, not
by the client API. In both cases the functionality is irrelevant for an
application that just plays audio through PulseAudio.

The stubs are shipped in `sharun+helper-libs-<arch>.tar` under `stubs/`.
`quick-sharun` decides per application whether to use them; if it does not,
it deletes the directory. `stubs/` is a subdirectory on purpose: sharun
automatically `LD_PRELOAD`s every *file* in `sharun-preload/`, but not files
in subdirectories. At runtime `lib.path` still makes the directory available
as a normal library path.

## Layout

```
lib/stubs/
├── Makefile
├── systemd/
│   ├── custom.txt      # symbols implemented by hand
│   └── overrides.c
├── sndfile/
│   ├── custom.txt
│   └── overrides.c
└── tools/
    ├── generate-manifests.sh # real libs+headers -> both symbols.tsv
    ├── gen-manifest.sh       # one library -> symbols.tsv
    ├── gen-stub.sh           # symbols.tsv -> C file + version script
    └── check-stub.sh         # built .so -> compare exports with symbols.tsv
```

`systemd/symbols.tsv` and `sndfile/symbols.tsv` are **not** committed. They
are generated from the real libraries by the release CI (or by
`make -C lib/stubs manifests` on an Arch host) and passed to the build as
an artifact. The build itself only needs a C compiler, `awk` and `readelf`;
the check uses `readelf`, `comm` and the generated manifests.

## Building

The manifests must be present first. On CI they come from the `manifests`
job; locally generate them on Arch (or in an Arch container):

```sh
make -C lib/stubs manifests   # needs make, systemd, libsndfile, gcc, binutils
make -C lib/stubs
make -C lib/stubs check
```

Cross compile the same way as the preload helpers:

```sh
make -C lib/stubs \
    CC="zig cc -target aarch64-linux-gnu.2.31" \
    LDFLAGS=-s
```

Both stubs are linked with `-nostdlib -Wl,-z,defs`, so they have no
`DT_NEEDED` entries and no `GLIBC_*` version requirements. That also avoids
the ppc64 ELFv1/ELFv2 glibc stub problem entirely.

## How the manifests are generated

`tools/generate-manifests.sh` is the only part that needs the real
libraries and their headers. It uses `readelf --dyn-syms` on the
`.so` files for the exact exported symbols and version nodes, and
`gcc -aux-info` on the public headers for the return types. It writes
`systemd/symbols.tsv` and `sndfile/symbols.tsv`, both gitignored.

The release CI runs it in a current Arch container before the build:

```sh
docker run --rm -v "$PWD:/src" -w /src archlinux:latest sh -ec '
  pacman -Syu --noconfirm --needed make systemd libsndfile gcc binutils
  make -C lib/stubs manifests
'
```

The two manifests are uploaded as the `stub-manifests` artifact and each
build job extracts them before compiling the stubs for its architecture.
Nothing has to be committed or updated by hand: a new systemd or
libsndfile release is picked up the next time the CI runs.

For local testing the script accepts explicit paths:

```sh
SYSTEMD_LIB=... SNDFILE_LIB=... \
SYSTEMD_INCLUDE_DIR=... SNDFILE_INCLUDE_DIR=... \
sh lib/stubs/tools/generate-manifests.sh
```

If a new symbol needs a specific behaviour instead of the generic
`-ENOSYS`/`NULL`, add it to `custom.txt` and `overrides.c`. The generator
fails if a listed custom symbol disappears from the manifest, so a rename
or removal is caught by the build.

The generated C and version script are not checked in either; the build
recreates them in `out/gen/`.

## What the stubs do

`libsystemd`:

- `sd_booted()` returns 0.
- `sd_listen_fds()` / `sd_listen_fds_with_names()` return 0; socket
  activation is not emulated.
- `sd_is_socket*()`, `sd_is_fifo()`, `sd_is_special()`, `sd_is_mq()` return
  0 (not a match).
- `sd_notify*()`, `sd_watchdog_enabled()`, `sd_journal_send*()`,
  `sd_journal_print*()`, `sd_journal_perror*()` return 0. Log messages are
  dropped and notifications are not sent.
- Everything else returns `-ENOSYS`, `NULL`, or zero, depending on the
  return type. `sd_bus`, `sd_event`, `sd-login` and journal reads therefore
  fail cleanly.

`libsndfile`:

- `sf_open*()` returns `NULL`.
- `sf_read*()` / `sf_write*()` return 0.
- `sf_seek()` returns -1.
- `sf_error()`, `sf_command()`, `sf_format_check()`, `sf_close()` and the
  error helpers return error/no-op values.
- `sf_strerror()` / `sf_error_number()` / `sf_version_string()` return
  `"libsndfile stub"`.

Both stubs are only meant for applications that do not use the real
functionality. `quick-sharun` should gate the libsndfile stub in particular,
because reading WAV/FLAC/Ogg files is a real feature of many applications.

## Versioning

The symbol versions mirror the real libraries:

- libsystemd: `LIBSYSTEMD_209` ... `LIBSYSTEMD_262` (one node per ABI
  version that introduced new symbols).
- libsndfile: everything is under `libsndfile.so.1.0`, with the base node
  `libsndfile.so.1` created automatically from the SONAME.

`check-stub.sh` compares `name@@version` pairs, so a stub never silently
drops a symbol that a linked library may need.
