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

## What this fork removes

- **`lib4bin`**: Has been removed. Use [quick-sharun](https://github.com/pkgforge-dev/Anylinux-AppImages/blob/main/useful-tools/quick-sharun.sh) instead.

- `xdg-open` wrapper: Has been removed. `quick-sharun` uses [anylinux.so](https://github.com/pkgforge-dev/Anylinux-AppImages/blob/main/useful-tools/lib/anylinux.c) which fixes the same issues that the wrapper did and better. (Works on all external binaires, not just `xdg-open`).

- **`sharun-aio`**: The all-in-one binary with bundled `lib4bin` dependencies is removed.

- **`sharun-lite`**: Removed.

- **wrappe integration**: No `--with-wrappe`. Use `quick-sharun --make-static-bin` instead.

- **Python packing with uv**: No `--with-python` support for embedding python/pip packages. `quick-sharun` only supports deploying the system python installation due to many bugs with uv python.

- **Strace mode**: No `strace` for library detection at runtime. `quick-sharun` uses `LD_DEBUG=libs` instead (reduces overdeployment of libraries).

