<p align="center">
  <img alt='LIVI' src='docs/images/banner.png' width="1200" />
</p>

# LIVI – Linux In-Vehicle Infotainment

LIVI is an open-source **Apple CarPlay and Android Auto head unit**.

It is a standalone cross-platform Electron head unit with hardware-accelerated video decoding, low-latency audio, multitouch + D-Pad navigation, and support for very small embedded/OEM displays.

Connectivity:

- **Android Auto** (wired) on all platforms
- **Android Auto** (wireless) on Linux
- **Android Auto** (wired & wireless) via Carlinkit dongle on all platforms
- **CarPlay** (wired & wireless) via Carlinkit dongle on all platforms

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
> LIVI requires **WebGL2 or WebGPU support**.

## Raspberry Pi OS

```bash
curl -fL -o install.sh https://raw.githubusercontent.com/f-io/LIVI/main/scripts/install/pi/install.sh
chmod +x install.sh
./install.sh
```

> Raspberry Pi OS **Trixie or newer** is required for WebGL2 support.

The `install.sh` script performs the following tasks:

1. checks for required tools: curl and xdg-user-dir
2. configures udev rules to ensure proper access rights for the CarPlay dongle
3. downloads the latest LIVI AppImage
4. creates an autostart entry so the application launches automatically on boot
5. creates a desktop shortcut for easy access

_This install script is not actively tested on other Linux distributions._

## Linux (x86_64)

This AppImage has been tested on Debian Trixie (13) with Wayland, Ubuntu/Kubuntu 25 and Fedora 44. No additional software is required, just download the `-x86_64.AppImage` and make it executable:

```bash
chmod +x LIVI-*-x86_64.AppImage
```

On first launch, LIVI will detect if the udev rule for the USB dongle is missing and prompt you to install it automatically.

> **Ubuntu / Kubuntu users:** Due to AppArmor restrictions, use the `.deb` package instead of the AppImage. The `.deb` automatically configures all required permissions. Alternatively, the AppImage can be started with `--no-sandbox` as a workaround.

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

> [!IMPORTANT]
> The Windows build is provided on a **best-effort basis**.
> Windows is **not a primary target platform** of this project and receives limited testing.
>
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

- **Python 3.x** (for native module builds via `node-gyp`)
- **build-essential** (Linux: includes `gcc`, `g++`, `make`, etc.)
- **libusb-1.0-0-dev** (required for `node-usb`)
- **libudev-dev** (optional but recommended for USB detection on Linux)
- **fuse** (required to run AppImages)

### Clone & Build

```bash
git clone --branch main --single-branch https://github.com/f-io/LIVI.git \
  && cd LIVI \
  && corepack enable \
  && corepack install

# Install dependencies from lockfile
pnpm run install:ci

# --- Build targets ---

# Linux x86_64 (AppImage)
pnpm run build:linux

# Linux ARM64 (AppImage)
pnpm run build:armLinux

# macOS (arm64 dmg)
pnpm run build:mac
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

<p align="center">
  <img src="docs/images/maps.png" alt="Maps (Cluster Stream)" width="42%" align="top" />
  &emsp;
  <img src="docs/images/telemetry.png" alt="Telemetry" width="42%" align="top" />
</p>

## Credits

See [CREDITS](CREDITS.md) for acknowledgements and prior art.

## Disclaimer

_Apple and CarPlay are trademarks of Apple Inc. Android and Android Auto are trademarks of Google LLC. This project is not affiliated with or endorsed by Apple or Google. All product names, logos, and brands are the property of their respective owners._

## License

This project is licensed under the MIT License.
