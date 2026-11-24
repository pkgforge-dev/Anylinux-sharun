# Anylinux-sharun

Fork of [VHSgunzo/sharun](https://github.com/VHSgunzo/sharun). Run dynamically linked ELF binaries everywhere (musl and glibc).

This fork is used by the [Anylinux-AppImages](https://github.com/pkgforge-dev/Anylinux-AppImages) project. It is intended for AppImage deployment only.

## What this fork adds

- **Extra architectures**: Builds for `riscv64`, `loongarch64`, `powerpc64` (big-endian) and `ppc64le` in addition to `x86_64` and `aarch64`. Uses a forked [userland-execve](https://github.com/pkgforge-dev/userland-execve-rust) with support for these architectures.

- **`SHARUN_MESA_PATH`**: Point to an external mesa installation (with `lib/` and `share/` subdirs). **Allows switching mesa versions at runtime.**

- **bwrap-wrapper**: When `sharun` is invoked as `bwrap`, it intercepts `bwrap` arguments to preserve essential paths and env variables (`$APPDIR`, `/tmp`, `/proc`, `$SHARUN_DIR`, `$PATH`). Rewrites hardcoded command paths to their AppDir equivalents. Falls back to system `bwrap` if real bwrap wasn't deployed. This lets applications that sandbox themselves with bwrap (example WebKitGTK) work correctly as AppImage.

- **`gio-launch-desktop` handler**: When `sharun` is hardlinked as `gio-launch-desktop`, it sets `GIO_LAUNCHED_DESKTOP_FILE_PID` and launches the target. Required for AppImages that rely on GIO-based `.desktop` file launching.

- **`AppRun.sh` support**: If an `AppRun.sh` exists in the sharun directory and sharun is hardlink as the `AppRun`, it executes `AppRun.sh` with the `dash` in the sharun directory if there is one, otherwise with any `sh`/`bash` found in the system or in `PATH`. With a `dash` there, **the `AppRun` has no `/bin/sh` dependency.** Every `sharun+helper-libs-<arch>.tar` has a static [dash](http://gondor.apana.org.au/~herbert/dash/) in it (`dash`), built by `tools/build-dash.sh` (BSD-3-Clause, the notice is in the tar as `dash-COPYING` and in the repo at `licenses/dash-COPYING`). It is dash 0.5.13.5 with `tools/dash-pattern-replace.patch`, which adds `${var/pat/str}`, `${var//pat/str}`, `${var/#pat/str}` and `${var/%pat/str}` like in bash (`tools/test-shell.sh` checks them against output made with bash). Otherwise it is plain POSIX sh: no `[[ ]]`, `${var:off:len}`, `source` or `function`, `${@/pat/str}`, `${@/#pat/str}` and `${@/%pat/str}` only change the first positional parameter (`//` changes all), and `echo -e` prints the `-e`, these do not give an error. Hooks must use the host `sed`, `awk` and `ln`, dash has no applets, and a hook that runs `sh -c ...` itself still gets the shell found in `PATH`.

- **Old kernel compatibility layer** (`x86_64` and `aarch64`): Provided by the standalone [old-linux-kernel-compat-layer](https://github.com/pkgforge-dev/old-linux-kernel-compat-layer) crate (originally developed here and split out so other projects can reuse it). Runs AppImages built against a modern glibc (e.g. an Arch Linux `--enable-kernel=4.4` build) on much older kernels:
  - **`x86_64`**: down to **Linux 2.6.8** (Debian 3.1), handling `SIGILL` (`XGETBV`) and missing syscalls (`futex` bitset/realtime, pre-2.6.23 `O_CLOEXEC`, `pipe2`, the `*at` family, `statx`, `getrandom`, `ppoll`, `epoll_pwait`, `epoll_create1`, `prlimit64`, `pwritev`/`preadv`, `F_DUPFD_CLOEXEC`, `eventfd`/`eventfd2`, `timerfd_*`, `termios2` ioctls, newer clock IDs, pre-2.6.19 echoed syscall numbers, and Linux 2.6.12 duplicate syscall-exit stops).
  - **`aarch64`**: down to **Linux 3.11** (Ubuntu 13.10), handling `getrandom`, `statx` (via `NT_ARM_SYSTEM_CALL` on 3.19+ and `svc #0` replay on 3.11–3.18), and `SIGILL` on `MRS` of EL1 CPU ID registers.

  It is enabled automatically on kernels older than 4.0 or when `statx` is missing; set `SHARUN_OLD_KERNEL_COMPAT=1` (or `OLD_KERNEL_COMPAT=1`) to force it on and `=0` to disable. `SHARUN_OLD_KERNEL_COMPAT_DEBUG=1` prints each translation to stderr, and `SHARUN_OLD_KERNEL_COMPAT_DEBUG_ALL=1` also prints every syscall and its result. On kernels with seccomp-bpf (`x86_64` 3.5+, `aarch64` 3.19+) only affected syscalls are intercepted (this sets `no_new_privs`, so setuid helpers cannot gain privileges); older kernels fall back to tracing every syscall with ptrace. 32-bit processes are traced but never translated. The tracer is installed from `AppRun` (never from `bin/*` hardlinks) or via `sharun --compat-exec CMD [ARGS...]`, and skips starting a second tracer when `OLD_KERNEL_COMPAT_ACTIVE=1` is already set by an outer runtime. See [old-linux-kernel-compat-layer](https://github.com/pkgforge-dev/old-linux-kernel-compat-layer) for the full list of translations and limits.

- **Directory structure**: Uses `lib`/`lib32` directly instead of `shared/lib`/`shared/lib32`. Fixes libraries that look for a relative `../share` directory and can't find it.
- **Payload directory**: the real programs of the `bin/<name>` hardlinks can live in `.bin/<name>` instead of `shared/bin/<name>`, sharun looks in `.bin` first. In `.bin` a program keeps its name in `/proc/self/exe` (and in `getauxval(AT_EXECFN)`), and `../lib` and `../share` resolve to the AppDir like in a normal install. An AppDir that only has `.bin`, without `shared`, works. A program that re-execs `/proc/self/exe` from a memfd (`runc` and `crun` do it) comes back named after that memfd, so when the exe name finds no payload sharun falls back to the basename of the caller's `argv[0]`.

- **Additional env vars**: Auto sets `LADSPA_PATH`, `FREI0R_PATH`, `MLT_REPOSITORY`, `MLT_PROFILES_PATH`, `MLT_PRESETS_PATH`, `GS_LIB`, `OPENSSL_CONF`, `QT_XKB_CONFIG_ROOT`, `PEAS_PLUGIN_LOADERS_DIR` and likely more in the future.

- **Bun workaround**: Detects Bun binaries and uses alternative execution paths so they run correctly via temp dynamic linker in `/tmp`. (These break when executed with the dynamic linker directly).

- **`sharun-preload` dir**: Every library found in `$SHARUN_DIR/lib/sharun-preload/` (or `lib32/sharun-preload/` for 32-bit binaries) is preloaded automatically, no `.preload` file needed. The classic `.preload` file method keeps working and its entries are resolved by name, so it can also reference libraries that live inside `sharun-preload/`. Files that are not `*.so` are ignored, and `path-mapping.so` is only preloaded when `PATH_MAPPING` is set, which allows shipping the library unconditionally.

- **Prebuilt helper libraries**: The preload libraries used by [quick-sharun](https://github.com/pkgforge-dev/Anylinux-AppImages/blob/main/useful-tools/quick-sharun.sh) originally lived in the [Anylinux-AppImages](https://github.com/pkgforge-dev/Anylinux-AppImages) repo and were compiled on the host at deployment time. They are now kept here (see `lib/`) and built by the CI with `zig cc` against a **glibc 2.31** floor (`2.36` for `loongarch64`, the first glibc version that supports that architecture). This guarantees they load inside any AppImage regardless of the glibc that was deployed, instead of depending on whatever glibc the CI host happened to have. `ppc64` (big-endian) is the exception: it is ELFv2 but zig's `powerpc64-linux-gnu` glibc stub is the ELFv1 one, which archlinuxpower's glibc does not define, so the ppc64 preloads are linked against a big-endian build of zig's ppc64le ELFv2 stub instead (see https://github.com/pkgforge-dev/Anylinux-sharun/issues/11). The libraries:

  - `anylinux.so` - main preload library: unsets problematic environment variables for child/external processes, restores portable home/config/data/cache dirs, fixes broken host locales, redirects `bindtextdomain` to the bundled locales, forces NSS to only use bundled modules, can block libraries from being dlopened with `ANYLINUX_DO_NOT_LOAD_LIBS` and can change the running program name with `OVERRIDE_ARGV0`.
  - `gtk-fix-nonsense.so` - forces the GTK window class / application id to `GTK_WINDOW_CLASS`, fixing broken desktop integration in Wayland where GNOME uses a different window class than in X11. It also fixes `gi_repository_require_private()` / `g_irepository_require_private()`, which only search the private directory they are given: a `/usr` prefix is remapped to `$APPDIR` so apps that look up a compiled-in libdir find the typelibs bundled in the appimage instead of the host copy, and the lookup falls back to the normal typelib search path (`GI_TYPELIB_PATH`) when the bundle misses. The host private directory is never used; only that fallback can still reach the default host girepository dir. Safe to preload into applications with or without GTK, GLib or glycin.
  - `glycin-fix.so` - disables the bwrap sandbox of GNOME's glycin image loader, which never works inside an AppImage because glycin incorrectly binds AppImage paths to bwrap, resulting in crashes. Only intended for applications that ship real glycin (`libglycin-*`), quick-sharun preloads it only for those, there is nothing to fix in [glycin-ng](https://github.com/QaidVoid/glycin-ng) based applications since it has a working sandbox. Glycin is only reached via `dlsym`/`dlopen(RTLD_NOLOAD)`, so the library does not affect applications that do not make use of glycin at all. Note that with the library preloaded `dlsym(RTLD_DEFAULT, "gly_loader_new")` returns a non-NULL wrapper even before glycin is loaded in, callers must check the returned loader for NULL.
  - `path-mapping.so` - vendored from [pathmap](https://github.com/VHSgunzo/pathmap), only the preload library, the standalone tracer binary is not built. Maps hardcoded paths at runtime with `PATH_MAPPING`/`PATHMAP_*` env variables, sharun only preloads it when `PATH_MAPPING` is set and it replaces the git clone + C compiler build that deployments used to carry out.

  The `stubs/` subdirectory holds two dummy libraries that are not preloaded: `libsystemd.so.0` and `libsndfile.so.1`. They export the complete ABI of the real libraries but answer with safe "not available" values, so applications that only get them transitively through `libpulse`/`libdbus` do not need to ship systemd or the libsndfile codec chain. See [`lib/stubs/README.md`](lib/stubs/README.md) for what is implemented and how the manifests are generated. `quick-sharun` decides per application whether to use them and deletes the directory otherwise.

  Each release contains, per architecture:

  - `sharun-$ARCH` - the sharun binary.
  - `sharun+helper-libs-$ARCH.tar` - sharun plus the prebuilt libraries in a single tar (`sharun`, `anylinux.so`, `gtk-fix-nonsense.so`, `glycin-fix.so`, `path-mapping.so`, and the `stubs/` directory), so consumers get everything with one download and no C compiler is needed on the build host.
  - A `.sha256` checksum file for each of the released assets.

## What this fork removes

- **`lib4bin`**: Has been removed. Use [quick-sharun](https://github.com/pkgforge-dev/Anylinux-AppImages/blob/main/useful-tools/quick-sharun.sh) instead.

- `xdg-open` wrapper: Has been removed. `quick-sharun` uses [anylinux.so](https://github.com/pkgforge-dev/Anylinux-sharun/blob/main/lib/anylinux.c) which fixes the same issues that the wrapper did and better. (Works on all external binaries, not just `xdg-open`).

- **`sharun-aio`**: The all-in-one binary with bundled `lib4bin` dependencies is removed.

- **`sharun-lite`**: Removed.

- **wrappe integration**: No `--with-wrappe`. Use `quick-sharun --make-static-bin` instead.

- **Python packing with uv**: No `--with-python` support for embedding python/pip packages. `quick-sharun` only supports deploying the system python installation due to many bugs with uv python.

- **Strace mode**: No `strace` for library detection at runtime. `quick-sharun` uses `LD_DEBUG=libs` instead (reduces overdeployment of libraries).

