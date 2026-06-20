<p align="center">
  <img alt='LIVI' src='docs/images/banner.png' width="1200" />
</p>

# LIVI – Linux In-Vehicle Infotainment

LIVI is an open-source **Apple CarPlay and Android Auto head unit**.

It is a standalone cross-platform Electron head unit with a native, zero-copy GStreamer video pipeline and hardware-accelerated decoding on Linux (including the Raspberry Pi 4 and 5), macOS and Windows, low-latency audio, multitouch + D-Pad navigation, and support for very small embedded/OEM displays.

## Native Connectivity

- **Android Auto** (wired) on all platforms
- **Android Auto** (wireless) on Linux

## Dongle-based Connectivity

- **Android Auto** (wired & wireless) on all platforms
- **Apple CarPlay** (wired & wireless) on all platforms

> **Supported USB adapters (for CarPlay):** Carlinkit **CPC200-CCPA** (wireless/wired) and **CPC200-CCPW** (wired)

## Project Status

![Release](https://img.shields.io/github/v/release/f-io/LIVI?label=release)
![Main Version](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-version.json)
![TS Main](https://img.shields.io/github/actions/workflow/status/f-io/LIVI/typecheck.yml?branch=main&label=TS%20main)
![Build Main](https://img.shields.io/github/actions/workflow/status/f-io/LIVI/build.yml?branch=main&label=build%20main)
![Coverage Main](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-coverage-main.json)
![Coverage Renderer](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-coverage-renderer.json)

## Installation

> [!IMPORTANT]
> LIVI requires **OpenGL ES 3.x**.

## Raspberry Pi OS

> [!NOTE]
> The Pi 4, CM 4, Pi 5 and CM 5 require Trixie (Debian 13) for OpenGL ES 3.x. Pi 3 and earlier use the VideoCore IV GPU, which only supports OpenGL ES 2.0 and is therefore unsupported.

```bash
curl -fL -o install.sh https://raw.githubusercontent.com/f-io/LIVI/main/scripts/install/pi/install.sh
chmod +x install.sh
./install.sh
```

The `install.sh` script performs the following tasks:

1. checks for required tools: curl, xdg-user-dir and pkexec
2. downloads the latest LIVI AppImage
3. creates an autostart entry so the application launches automatically on boot
4. creates a desktop shortcut for easy access
5. applies the Raspberry Pi HEVC decoder patch if the system GStreamer is affected (see note below)

> [!NOTE]
> **Raspberry Pi HEVC (1080p) hardware decode** uses the system GStreamer `v4l2codecs` plugin. GStreamer **1.26.x before 1.26.11** (currently shipped by Raspberry Pi OS) has a SAND-crop bug that breaks zero-copy at 1080p and leaves the main video layer black. The installer detects an affected version and rebuilds the patched plugin from the distribution source automatically. If you do not use the installer, apply it manually from a LIVI checkout on the Pi:
>
> ```bash
> bash scripts/gstreamer/patch-pi-v4l2codecs.sh
> ```

On first launch, LIVI detects if the udev rule for USB access is missing and prompts you to install it. The rule grants USB access to connected Android phones (for wired Android Auto) and to the USB dongle.

_This install script is not actively tested on other Linux distributions._

## Linux (x86_64)

This AppImage has been tested on Debian Trixie (13) with Wayland, Ubuntu 26.04 and Fedora 44 (GMNOME).

```bash
chmod +x LIVI-*-x86_64.AppImage
```

On first launch, LIVI detects if the udev rule for USB access is missing and prompts you to install it. The rule grants USB access to connected Android phones (for wired Android Auto) and to the USB dongle.

> **Hardware video decode (optional):** LIVI uses the system VA-API driver for GPU video decode (it is not bundled, since it must match your GPU and kernel). Most desktops ship it, a minimal install may not. Without it LIVI still works via software decode. For HW decode install the driver for your GPU and verify with `vainfo`: `i965-va-driver` (older Intel, e.g. Broadwell), `intel-media-va-driver` (Gen9+ Intel), `mesa-va-drivers` (AMD).

> **Ubuntu / Kubuntu users:** On Ubuntu 24.04 AppArmor blocks the Chromium sandbox for AppImages, start it with `--no-sandbox` as a workaround. Ubuntu 24.10 and newer run the AppImage out of the box.

## Mac (arm64)

Download the `-arm64.dmg`, open it, and drag **LIVI.app** into Applications.

When launching the app for the first time, macOS may block it.
In that case:

1. Try to open the app once (it will be blocked)
2. Go to **System Settings → Privacy & Security**
3. Scroll down and click **“Open Anyway”**
4. Confirm the dialog

After this, the app will launch normally and future updates will work without additional steps.


## Windows (x64)

> [!NOTE]
> The Windows build is provided on a **best-effort basis**. Windows is **not a primary target platform** of this project and receives limited testing.
> It is mainly intended for development, experimentation, and desktop testing.

### USB Driver Requirement

The Carlinkit dongle requires a compatible **WinUSB (winusb.sys)** driver on Windows.
You can install it using a tool such as **Zadig** (libwdi): https://github.com/pbatard/libwdi/releases

Steps:

1. Plug in the Carlinkit dongle
2. Start Zadig
3. Select the dongle from the device list
4. Install the **WinUSB (winusb.sys)** driver

## Build Environment

![Node](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-node.json)
![pnpm](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-pnpm.json)
![electron](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-electron.json)
![chrome](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-electron-date.json)
![release](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-electron-chromium.json)
![gstreamer](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/LIVI/version/.github/badges/main-gstreamer.json)

### System Requirements (build)

Make sure the following packages and tools are installed on your system before building:

- **Node.js 24.x** (with `corepack` for `pnpm`)
- **Python 3.x** (for native module builds via `node-gyp`)
- **build-essential** (Linux: includes `gcc`, `g++`, `make`, etc.)
- **libgstreamer1.0-dev** + **libgstreamer-plugins-base1.0-dev** (required to build the `gst-video` addon)
- **meson** (≥ 1.4), **ninja**, **pkg-config**, **bison**, **cmake** and the wlroots/EGL stack: **libwayland-dev**, **wayland-protocols**, **libxkbcommon-dev** (≥ 1.8.0), **libpixman-1-dev**, **libcairo2-dev**, **libegl-dev** / **libgles-dev** / **libgbm-dev** / **libffi-dev** / **libexpat1-dev** (Linux only: to build the embedded wlroots compositor)
- **fuse** (required to run AppImages)

On Debian/Ubuntu/Raspberry Pi OS, install everything with:

```bash
sudo apt-get update
sudo apt-get install -y git build-essential python3 python3-dev python3-pip \
  pkg-config bison ninja-build cmake \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libegl-dev libgles-dev libgbm-dev libffi-dev libexpat1-dev \
  libwayland-dev wayland-protocols libxkbcommon-dev libpixman-1-dev libcairo2-dev
pip3 install --user --break-system-packages 'meson>=1.4'
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo apt-get install -y nodejs
sudo corepack enable
```

On Fedora, install everything with:

```bash
sudo dnf install -y git gcc gcc-c++ make python3 python3-devel \
  pkgconf-pkg-config systemd-devel \
  gstreamer1-devel gstreamer1-plugins-base-devel \
  meson ninja-build bison cmake \
  wlroots-devel wayland-devel wayland-protocols-devel libxkbcommon-devel \
  pixman-devel cairo-devel \
  mesa-libEGL-devel mesa-libGLES-devel mesa-libgbm-devel libffi-devel expat-devel \
  fuse fuse-libs
curl -fsSL https://rpm.nodesource.com/setup_24.x | sudo bash -
sudo dnf install -y nodejs
sudo corepack enable
```


On macOS, the `gst-video` addon links against the **GStreamer.framework**. Install
both the runtime and development packages (matching versions) from
[gstreamer.freedesktop.org](https://gstreamer.freedesktop.org/download/#macos)
before building. `node-gyp` discovers it via `pkg-config` under
`/Library/Frameworks/GStreamer.framework`.

### Clone & Build

```bash
# Git clone
git clone --branch main --single-branch https://github.com/f-io/LIVI.git \
  && cd LIVI

# Install dependencies from lockfile
pnpm run install:ci

# --- Build targets ---

# Linux x86_64 (AppImage)
pnpm run build:linux

# Linux ARM64 (AppImage)
pnpm run build:armLinux

# macOS (dmg)
pnpm run build:mac:arm64           # Apple Silicon
pnpm run build:mac:x64             # Intel
```

## Dashboard

The Dashboard is currently in an early stage. While the IPC/socket telemetry payload already supports many signals, the UI exposes only a small subset. Widgets and layouts will be extended over time.

### Telemetry CLI (local)

To push test data into a running LIVI, use the CLI in `scripts/tools`. The full
field list and routing (Dash / AA / Dongle) lives in
`src/main/shared/types/Telemetry.ts`.

```bash
pnpm -C scripts/tools install

# Realistic all-fields demo push
pnpm -C scripts/tools run telemetry:demo

# Send single fields or blocks ad-hoc
pnpm -C scripts/tools run telemetry:set fuelPct=4 rangeKm=38
pnpm -C scripts/tools run telemetry:set gps.lat=53.5912 gps.lng=10.015
pnpm -C scripts/tools run telemetry:set _repeatMs=1000 speedKph=90 rpm=2500
```

<p align="center">
  <img src="docs/images/dash.png" alt="Dashboard" width="70%" />
</p>

## View and Safe Area

Stream resolution, view area insets, and safe area can be configured independently for the main and cluster streams. This is supported for Android Auto as well as CarPlay.

### Main Stream
Video: 1280x720 - View Area: 0/0/100/0 (T/B/L/R) - Safe Area: 100/100/100/100 (T/B/L/R) - Draw Outside: true
<p align="center">
  <img src="docs/images/area/main_safe_area_view_area_aa.png" alt="Safe area main stream Android Auto" width="70%" />
</p>

### Cluster Stream
Video: 1280x720 - View Area: 0/0/0/0 (T/B/L/R) - Safe Area: 60/20/350/350 (T/B/L/R)
<p align="center">
  <img src="docs/images/area/dash_safe_area_aa.png" alt="Safe area cluster stream Android Auto" width="70%" />
</p>



## Multi-Display

LIVI can run as multiple windows at once, each placeable on its own physical display.
The Dash and Aux windows are freely assignable and can show the Dashes, the reverse camera or the media player. Assignment is not exclusive: any feature can be shown on one, several, or all windows at the same time.

Configure each window under Settings → Window Settings
(Main Screen / Dash Screen / Aux Screen), and assign features under
Settings → General → Tab Settings.

<p align="center">
  <img src="docs/images/multi-display/dash.png" alt="Dash Screen" width="70%" />
</p>

<p align="center">
  <img src="docs/images/multi-display/auxilary.png" alt="Aux Screen" width="34%" align="top" />
  <img src="docs/images/multi-display/livi.png" alt="Main Screen" width="34%" align="top" />
</p>

## Images

<p align="center">
  <img src="docs/images/carplay.png" alt="CarPlay" width="42%" align="center" />
  &emsp;
  <img src="docs/images/aa.png" alt="Android Auto" width="42%" align="center" />
</p>

<p align="center">
  <img src="docs/images/media.png" alt="Media" width="42%" align="top" />
  &emsp;
  <img src="docs/images/settings.png" alt="Settings" width="42%" align="top" />
</p>

## Credits

See [CREDITS](CREDITS.md) for acknowledgements and prior art.

## Disclaimer

_Apple and CarPlay are trademarks of Apple Inc. Android and Android Auto are trademarks of Google LLC. This project is not affiliated with or endorsed by Apple or Google. All product names, logos, and brands are the property of their respective owners._

## License

LIVI is free software, licensed under the **GNU General Public License v3.0 or later** (`GPL-3.0-or-later`). See [LICENSE](LICENSE) for the full text.

Copyright (C) 2025 Lasse Heitgres

You are free to use, study, share, and modify LIVI. If you distribute it or a modified version, you must pass on the same freedoms and make the corresponding source available under the GPL. It comes with NO WARRANTY, to the extent permitted by law.
