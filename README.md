<p align="center">
  <!-- Release -->
  <img alt="Release" src="https://img.shields.io/github/v/release/f-io/pi-carplay?label=release"> &nbsp;&nbsp;&nbsp;
  <!-- MAIN -->
  <img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/pi-carplay/version/.github/badges/main-version.json">
  <img alt="TS Main" src="https://img.shields.io/github/actions/workflow/status/f-io/pi-carplay/typecheck.yml?branch=main&label=TS%20main&style=flat">
  <img alt="Build Main" src="https://img.shields.io/github/actions/workflow/status/f-io/pi-carplay/build.yml?branch=main&label=build%20main&style=flat"> &nbsp;&nbsp;&nbsp;
  <!-- DEV -->
  <img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/pi-carplay/version/.github/badges/dev-version.json">
  <img alt="TS Dev" src="https://img.shields.io/github/actions/workflow/status/f-io/pi-carplay/typecheck.yml?branch=dev&label=TS%20dev&style=flat">
  <img alt="Build Dev" src="https://img.shields.io/github/actions/workflow/status/f-io/pi-carplay/build.yml?branch=dev&label=build%20dev&style=flat">
</p>

# <img src="assets/icons/linux/pi-carplay.png" alt="pi-carplay" width="25px" /> pi-carplay

pi-carplay brings Apple CarPlay and Android Auto to the Raspberry Pi, with support for Linux (ARM/x86) and macOS (ARM) as well. It is a standalone Electron app, optimized for embedded setups and ultra-low-resolution OEM displays.  

> **Requirements:** A Carlinkit **CPC200-CCPA** (wireless & wired) or **CPC200-CCPW** (wired only) adapter.

## Build Environment

![Node](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/pi-carplay/version/.github/badges/main-node.json)
![npm](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/pi-carplay/version/.github/badges/main-npm.json)
![electron](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/pi-carplay/version/.github/badges/main-electron-info.json)

## Images

<p align="center">
  <img src="documentation/images/carplay.png" alt="CarPlay" width="58%" />
</p>
<p align="center">
  <img src="documentation/images/carplay_no_phone.png" alt="No Phone" width="48%" align="top" />
  &emsp;
  <img src="documentation/images/media.png" alt="Media" width="48%" align="top" />
</p>
<p align="center">
  <img src="documentation/images/info.png" alt="Info" width="48%" align="top" />
  &emsp;
  <img src="documentation/images/settings.png" alt="Settings" width="48%" align="top" />
</p>

# Installation

## Raspberry Pi OS

```bash
curl -LO https://raw.githubusercontent.com/f-io/pi-carplay/main/setup-pi.sh
sudo chmod +x setup-pi.sh
./setup-pi.sh
```

The `setup-pi.sh` script performs the following tasks:

1. check for required tools: curl and xdg-user-dir
2. configures udev rules to ensure the proper access rights for the CarPlay dongle
3. downloads the latest AppImage
4. creates an autostart entry, so the application will launch automatically on boot
5. creates a desktop shortcut for easy access to the application

*Not actively tested on other Linux distributions.*

---

## Linux (x86_64)

This AppImage has been tested on Debian Trixie (13). No additional software is required — just download the `-x86_64.AppImage` and make it executable. Depending on your distro and how you run the app, you may need a udev rule to access the USB dongle. It presents as a composite (multi-class) USB device, and unlike single-class devices, its interfaces often require explicit permissions. The optional install script can create this rule for you.

```udev
SUBSYSTEM=="usb", ATTR{idVendor}=="1314", ATTR{idProduct}=="152*", MODE="0660", GROUP="plugdev"
```

```bash
chmod +x pi-carplay-*-x86_64.AppImage
```

---

## Mac (arm64)

Just download the `-arm64.dmg`, open it, and drag pi-carplay.app into Applications. Then remove the Gatekeeper quarantine once and launch the app.
This step is required for all non-Apple-signed apps and future in-app updates will preserve this state.

```bash
xattr -cr /Applications/pi-carplay.app
```

For microphone support, please install Sound eXchange (SoX) via brew.
```bash
brew install sox
```

---

## System Requirements (build)

Make sure the following packages and tools are installed on your system before building:

- **Python 3.x** (for native module builds via `node-gyp`)
- **build-essential** (Linux: includes `gcc`, `g++`, `make`, etc.)
- **libusb-1.0-0-dev** (required for `node-usb`)
- **libudev-dev** (optional but recommended for USB detection on Linux)
- **fuse** (required to run AppImages)

---

## Clone & Build

```bash
git clone --branch main --single-branch https://github.com/f-io/pi-carplay.git \
  && cd pi-carplay \
  && npm run install:clean \
  && npm run build \
  && npm run build:armLinux
```

---

## Android Auto

> **Provisioning not supported.** This app does **not** perform the Android Auto first-time provisioning/pairing flow.  
> Your phone must already be paired/enrolled **on the dongle**.

**How to provision AA on the dongle:**
1. Use the dongle with a regular head unit **or** the vendor’s mobile app to add your phone once.
2. After the dongle knows your phone, connect the dongle to pi-carplay — it will attach without running provisioning again.

---

## Disclaimer

_Apple and CarPlay are trademarks of Apple Inc. Android and Android Auto are trademarks of Google LLC. This project is not affiliated with or endorsed by Apple or Google. All product names, logos, and brands are the property of their respective owners._

## License

This project is licensed under the MIT License.
