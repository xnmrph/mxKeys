# Build toolchain

mxKeys builds ZMK firmware itself, with no Docker, Nix or user-installed Zephyr SDK.
Everything lives in the app's data directory and is pinned in
[`toolchain/manifest.toml`](../toolchain/manifest.toml). This page records how the
pieces were chosen, what they cost in size, and the trade-offs still open.

All numbers below were **measured** in session 3 (2026-11-23) on Linux x86-64, building
the owner's real config (`xnmrph/corne`: ZMK `main` + `urob/zmk-helpers` +
`urob/zmk-unicode`) for left (nice!view + Studio), right (nice!view) and settings reset.

## What's in it

| Component | Version | Why this one |
|---|---|---|
| Zephyr SDK (minimal + `arm-zephyr-eabi` only) | **0.16.9** | Exactly what ZMK's build image uses (`zmkfirmware/zmk-docker`, `4.1-branch`). SDK 0.17.4 **fails** to compile Zephyr 4.1 (`conflicting types for '__lock___libc_recursive_mutex'` in picolibc). Rule: pin the SDK ZMK pins. |
| Python | 3.12.16, python-build-standalone `install_only_stripped` | Self-contained, relocatable; stripped build is 33 MB / 105 MB instead of 65 MB / 223 MB. |
| Python packages | `west==1.5.0`, then Zephyr `requirements-base.txt` + nanopb `extra/requirements.txt` | nanopb's `grpcio-tools` bundles `protoc`, needed for ZMK Studio. *Not* `west packages pip --install`, which installs all Zephyr dev tooling (venv 285 MB vs 60 MB). |
| CMake | 3.31.12 | CMake 4 removed compatibility with very old `cmake_minimum_required` values some modules may still use; 3.31 is the safe choice for Zephyr 4.1. |
| Ninja | 1.13.2 | Publishes no checksums; we hash the release files ourselves. |
| dtc | — | **Not needed**: Zephyr 4.1 only runs dtc "if it was found", for extra warnings (`cmake/modules/dts.cmake`). |
| git | system, or MinGit 2.56.1 on Windows | `west` needs a git binary — see "Open: git" below. |

Build invocation (per target):

```
west build -p auto -s <ws>/zmk/app -d <build> -b nice_nano@2.0.0//zmk [-S studio-rpc-usb-uart] \
  -- -DZephyr_DIR=<ws>/zephyr/share/zephyr-package/cmake -DSHIELD="corne_left nice_view_adapter nice_view" \
     -DZMK_CONFIG=<repo>/config [-DZMK_EXTRA_MODULES=<repo>]
```

with `ZEPHYR_SDK_INSTALL_DIR`, `ZEPHYR_TOOLCHAIN_VARIANT=zephyr` and our tools first on
`PATH`. Two findings worth remembering:

- **`//zmk` board variant is required** on ZMK `main`: `nice_nano@2.0.0` selects Zephyr's
  stock board, which lacks ZMK's additions — the nice!view build fails with
  `undefined node label 'nice_view_spi'`. (The owner's build.yaml has this problem.)
- **No `west zephyr-export`**: it writes into the user's global `~/.cmake` package
  registry. We pass `-DZephyr_DIR` instead.

## Sizes (Linux x86-64)

| | Download | On disk, naive | On disk, mxKeys |
|---|---|---|---|
| Zephyr SDK minimal | 43 MB | 43 MB (installer for host tools) | ~0 (installer deleted) |
| arm-zephyr-eabi toolchain | 107 MB | 1.1 GB | **256 MB** (multilib trim) |
| Python | 33 MB (stripped) | 223 MB (unstripped) | 105 MB |
| venv | ~35 MB of wheels | 285 MB (`west packages pip`) | **60 MB** |
| CMake | 53 MB | 175 MB | ~110 MB (docs removed) |
| Ninja | 0.1 MB | 0.3 MB | 0.3 MB |
| West workspace (git, depth 1) | ~223 MB of git data | **3.0 GB** (58 projects) | **1.2 GB** (13 projects, allow-list) |
| Build dirs (3 targets) | — | ~125 MB | ~125 MB |
| **Total** | **≈ 500 MB** | **≈ 4.9 GB** | **≈ 1.85 GB** |

Times on the cloud dev machine: workspace fetch 74 s (full) → 23 s (allow-list); builds
19 s (left), 15 s (right), 9 s (settings reset).

### Trimming that is verified safe

Both trims produced **byte-identical** `zmk.bin` for all three targets compared to the
untrimmed setup:

1. **Multilib trim** — the GCC/newlib/picolibc toolchain ships libraries for every ARM
   core (`thumb/v6-m`, `v7e-m+fp/softfp`, `v8-m.main`, A-profile `arm/…`, …). A nice!nano
   (nRF52840, `-mcpu=cortex-m4 -mfloat-abi=hard -mfpu=fpv4-sp-d16`) uses only
   `thumb/v7e-m+fp/hard`. The controller catalog declares each controller's multilib; the
   installer keeps the union for supported controllers.
2. **Workspace allow-list** — a generated manifest keeps the user's projects and
   revisions and adds `name-allowlist` to ZMK's import (applied transitively to Zephyr's
   module list): `zephyr, cmsis, cmsis_6, hal_nordic, mbedtls, nanopb, lvgl,
   zmk-studio-messages, tinycrypt, segger`. Also verified with the screen enabled (LVGL)
   on both halves. User-added modules (zmk-helpers, …) are always kept.

### Further options (not done, with trade-offs)

| Option | Saves | Cost / risk |
|---|---|---|
| Don't download the SDK "minimal" bundle; ship its ~10 KB of CMake package files (Apache-2.0) in the app | 43 MB download (Linux; smaller elsewhere) | Must stay in sync with the pinned SDK version; easy to test. **Recommended later.** |
| Sparse/partial checkouts of big repos (LVGL ships demos/docs, 213 MB; hal_nordic 252 MB) | ~300 MB disk | west has no sparse checkout; we'd post-process checkouts and have to undo it before `west update`. Fragile. Not recommended. |
| Delete each repo's `.git` after checkout | ~220 MB | Breaks `west update`; re-fetch on every ZMK update. Not recommended. |
| Share one workspace across projects | already done (keyed by generated manifest) | — |
| Build without `west` (CMake directly, `-DZEPHYR_MODULES=…`) | Python deps for west only | Also removes the git requirement — see below. |

## Decisions

**Toolchain size vs completeness (ARCHITECTURE §6): implemented option (c)** — the
allow-listed workspace by default; if a build fails for a reason the log classifier
can't explain, the pipeline retries once with the full manifest. A "Use the full ZMK
workspace" switch can be exposed in Advanced settings.

## Open: git

`west update` (and `west build`'s module discovery) need a git binary.

- Linux: git is effectively always present; the app explains how to install it if not.
- Windows: the app downloads **MinGit** (official portable Git for Windows, 38 MB,
  sha256-pinned) when no git is installed. CI exercises this path
  (`MXKEYS_FORCE_BUNDLED_GIT=1`).
- macOS: git comes with Apple's Command Line Tools; without them the app asks the user to
  run `xcode-select --install` (a system dependency, against the brief's goal).

**Long-term option:** resolve the west manifest ourselves, fetch with libgit2 (already in
the app for cloning configs), and call CMake directly with `-DZEPHYR_MODULES=…`. That
removes git *and* west from the build entirely. Cost: re-implementing west's manifest
semantics (imports, allow/block-lists, groups, path prefixes) — a few hundred lines with
plenty of edge cases. **Recommendation:** do it once the rest of the app is stable,
starting with a "resolver equals `west list`" test on several real configs.

## Reproducibility

- Every download is sha256-verified against the manifest; the install directory is named
  after the manifest's hash, so changing a pin installs side by side.
- `west update` records `<project> <commit>` for every project in `mxkeys-lock.txt` in
  the workspace. Configs that follow a moving branch (`revision: main`) get a warning in
  the app; the lock tells which commit a build actually used.
- Python packages from the workspace's requirement files are not hash-locked yet (they
  are lower bounds like `PyYAML>=6.0`). **TODO:** a constraints file per manifest.

## Layout

```
<app data>/
  downloads/                      archives while downloading (deleted after install)
  toolchain/<manifest-hash>/
    zephyr-sdk/{cmake,sdk_version,arm-zephyr-eabi}
    python/  venv/  cmake/  ninja/  [git/]
    .done-<component>             install markers (resume after interruption)
  workspaces/<manifest-hash>/     west workspace (shared by configs with the same west.yml)
    config/west.yml               generated manifest
    mxkeys-lock.txt               resolved commits
  builds/<repo-hash>/{left,right,settings_reset}/
  firmware/<repo-hash>/{left,right,settings_reset}.uf2
```
