# Anylinux-sharun

Fork of [VHSgunzo/sharun](https://github.com/VHSgunzo/sharun). Run dynamically linked ELF binaries everywhere (musl and glibc).

This fork is used by the [Anylinux-AppImages](https://github.com/pkgforge-dev/Anylinux-AppImages) project. It is intended for AppImage deployment only.

## What this fork adds

- **Extra architectures**: Builds for `riscv64`, `loongarch64` and `powerpc64` (big-endian) in addition to `x86_64` and `aarch64`. Uses a forked [userland-execve](https://github.com/pkgforge-dev/userland-execve-rust) with support for these architectures.

- **`SHARUN_MESA_PATH`**: Point to an external mesa installation (with `lib/` and `share/` subdirs). **Allows switching mesa versions at runtime.**

- **bwrap-wrapper**: When `sharun` is invoked as `bwrap`, it intercepts `bwrap` arguments to preserve essential paths and env variables (`$APPDIR`, `/tmp`, `/proc`, `$APPDIR`, `$SHARUN_DIR`, `$PATH`). Rewrites hardcoded command paths to their AppDir equivalents. Falls back to system `bwrap` if real bwrap wasn't deployed. This lets applications that sandbox themselves with bwrap (example WebKitGTK) work correctly as AppImage.

- **`gio-launch-desktop` handler**: When `sharun` is hardlinked as `gio-launch-desktop`, it sets `GIO_LAUNCHED_DESKTOP_FILE_PID` and launches the target. Required for AppImages that rely on GIO-based `.desktop` file launching.

- **`AppRun.sh` support**: If an `AppRun.sh` exists in the sharun directory and sharun is hardlink as the `AppRun`, it executes `AppRun.sh` using any `sh`/`bash` found in `PATH` or the AppDir, **removes hard `/bin/sh` dependency from `AppRun`.**

- **Directory structure**: Uses `lib`/`lib32` directly instead of `shared/lib`/`shared/lib32`. Fixes libraries that look for a relative `../share` directory and can't find it.

- **Additional env vars**: Auto sets `LADSPA_PATH`, `FREI0R_PATH`, `MLT_REPOSITORY`, `MLT_PROFILES_PATH`, `MLT_PRESETS_PATH`, `GS_LIB`, `OPENSSL_CONF`, and `QT_XKB_CONFIG_ROOT` and likely more in the future.

- **Bun workaround**: Detects Bun binaries and uses alternative execution paths so they run correctly via temp dynamic linker in `/tmp`. (These break when executed with the dynamic linker directly).

- **Prebuilt helper libraries**: The preload libraries used by [quick-sharun](https://github.com/pkgforge-dev/Anylinux-AppImages/blob/main/useful-tools/quick-sharun.sh) originally lived in the [Anylinux-AppImages](https://github.com/pkgforge-dev/Anylinux-AppImages) repo and were compiled on the host at deployment time. They are now kept here (see `lib/`) and built by the CI with `zig cc` against a **glibc 2.31** floor (`2.36` for `loongarch64`, the first glibc version that supports that architecture). This guarantees they load inside any AppImage regardless of the glibc that was deployed, instead of depending on whatever glibc the CI host happened to have. The libraries:

  - `anylinux.so` - main preload library: unsets problematic environment variables for child/external processes, restores portable home/config/data/cache dirs, fixes broken host locales, redirects `bindtextdomain` to the bundled locales, forces NSS to only use bundled modules, can block libraries from being dlopened with `ANYLINUX_DO_NOT_LOAD_LIBS` and can change the running program name with `OVERRIDE_ARGV0`.
  - `gtk-fix-nonsense.so` - forces the GTK window class / application id to `GTK_WINDOW_CLASS`, fixing broken desktop integration in Wayland where GNOME uses a different window class than in X11. It also forces GNOME's glycin image loader to disable its bwrap sandbox, which never works inside an AppImage (because glycin incorrectly binds AppImage paths to bwrap, resulting in crashes), and switches gsettings to the `keyfile` backend when portable home/config mode is used. We don't recommend you deploy your application with GNOME glycin, instead use [glycin-ng](https://github.com/QaidVoid/glycin-ng) which is a much smaller alternative that has working sandbox. Glycin is only reached via `dlsym`/`dlopen(RTLD_NOLOAD)`, so the library is safe to preload into applications that do not use glycin at all.

  Each release contains, per architecture:

  - `sharun-$ARCH` - the sharun binary.
  - `sharun+helper-libs-$ARCH.tar` - sharun plus the prebuilt libraries in a single flat tar (`sharun`, `anylinux.so`, `gtk-fix-nonsense.so`), so consumers get everything with one download and no C compiler is needed on the build host.
  - A `.sha256` checksum file for each of the released assets.

## What this fork removes

- **`lib4bin`**: Has been removed. Use [quick-sharun](https://github.com/pkgforge-dev/Anylinux-AppImages/blob/main/useful-tools/quick-sharun.sh) instead.

- `xdg-open` wrapper: Has been removed. `quick-sharun` uses [anylinux.so](https://github.com/pkgforge-dev/Anylinux-sharun/blob/main/lib/anylinux.c) which fixes the same issues that the wrapper did and better. (Works on all external binaires, not just `xdg-open`).

- **`sharun-aio`**: The all-in-one binary with bundled `lib4bin` dependencies is removed.

- **`sharun-lite`**: Removed.

- **wrappe integration**: No `--with-wrappe`. Use `quick-sharun --make-static-bin` instead.

- **Python packing with uv**: No `--with-python` support for embedding python/pip packages. `quick-sharun` only supports deploying the system python installation due to many bugs with uv python.

- **Strace mode**: No `strace` for library detection at runtime. `quick-sharun` uses `LD_DEBUG=libs` instead (reduces overdeployment of libraries).

