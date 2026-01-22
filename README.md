<p align="center">
  <img alt='LIVI' src='docs/images/banner.png' width="1200" />
</p>

# LIVI – Linux In-Vehicle Infotainment

LIVI is an open-source **Apple CarPlay and Android Auto head unit for Linux**.

It is a standalone cross-platform Electron head unit with hardware-accelerated video decoding, low-latency audio, multitouch + D-Pad navigation, and support for very small embedded/OEM displays.

> **Supported USB adapters:** Carlinkit **CPC200-CCPA** (wireless/wired) and **CPC200-CCPW** (wired)

## Project Status

![Release](https://img.shields.io/github/v/release/f-io/livi?label=release)
![Main Version](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/livi/version/.github/badges/main-version.json)
![TS Main](https://img.shields.io/github/actions/workflow/status/f-io/livi/typecheck.yml?branch=main&label=TS%20main)
![Build Main](https://img.shields.io/github/actions/workflow/status/f-io/livi/build.yml?branch=main&label=build%20main)

## Dongle Firmware Feature Matrix

The available features depend on the firmware version running on the Carlinkit dongle.
Listed limitations are firmware-level restrictions and cannot be fixed by the application.

| Firmware Version | Nav Auto Switch | Call Auto Switch | Audio Metadata | Web Interface | Notes                                    |
| :--------------: | :-------------: | :--------------: | :------------: | :-----------: | :--------------------------------------- |
| 2025.10.15.1127  |       🟢        |        🟢        |       🟢       |      🟢       | Full feature set                         |
| 2025.02.25.1521  |       🔴        |        🟢        |       🟢       |      🟢       | No auto switching on navigation guidance |
| 2021.02.23.1758  |       🔴        |        🟡        |       🔴       |      🔴       | Limited protocol support                 |

### Legend

- `🟢` = Supported
- `🔴` = Not supported
- `🟡` = Unkown

### Notes

- The firmware updater may not detect devices running USB FW ≤ 2.0
- The dongle web interface does not support firmware updates; it requires the original APK

## Installation

### Raspberry Pi OS

```bash
curl -LO https://raw.githubusercontent.com/f-io/livi/main/setup-pi.sh
sudo chmod +x setup-pi.sh
./setup-pi.sh
```

The `setup-pi.sh` script performs the following tasks:

1. check for required tools: curl and xdg-user-dir
2. configures udev rules to ensure the proper access rights for the CarPlay dongle
3. downloads the latest AppImage
4. creates an autostart entry, so the application will launch automatically on boot
5. creates a desktop shortcut for easy access to the application

_Not actively tested on other Linux distributions._

### Linux (x86_64)

This AppImage has been tested on Debian Trixie (13). No additional software is required — just download the `-x86_64.AppImage` and make it executable. Depending on your distro and how you run the app, you may need a udev rule to access the USB dongle. It presents as a composite (multi-class) USB device, and unlike single-class devices, its interfaces often require explicit permissions.

```bash
sudo bash -c '
  RULE_FILE="/etc/udev/rules.d/99-LIVI.rules"
  USER_NAME="${SUDO_USER:-$USER}"

  echo "Creating udev rule for Carlinkit dongle (owner: $USER_NAME)"
  echo "SUBSYSTEM==\"usb\", ATTR{idVendor}==\"1314\", ATTR{idProduct}==\"152*\", " \
       "MODE=\"0660\", OWNER=\"$USER_NAME\"" \
    > "$RULE_FILE"

  echo "Reloading udev rules…"
  udevadm control --reload-rules
  udevadm trigger

  echo "Done."
'
```

```bash
chmod +x LIVI-*-x86_64.AppImage
```

### Mac (arm64)

Just download the `-arm64.dmg`, open it, and drag LIVI.app into Applications. Then remove the Gatekeeper quarantine once and launch the app.
This step is required for all non-Apple-signed apps and future in-app updates will preserve this state.

```bash
xattr -cr /Applications/LIVI.app
```

For microphone support, please install Sound eXchange (SoX) via brew.

```bash
brew install sox
```

## Build Environment

![Node](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/livi/version/.github/badges/main-node.json)
![npm](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/livi/version/.github/badges/main-npm.json)
![electron](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/livi/version/.github/badges/main-electron.json)
![chrome](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/livi/version/.github/badges/main-electron-date.json)
![release](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/f-io/livi/version/.github/badges/main-electron-chromium.json)

### System Requirements (build)

Make sure the following packages and tools are installed on your system before building:

- **Python 3.x** (for native module builds via `node-gyp`)
- **build-essential** (Linux: includes `gcc`, `g++`, `make`, etc.)
- **libusb-1.0-0-dev** (required for `node-usb`)
- **libudev-dev** (optional but recommended for USB detection on Linux)
- **fuse** (required to run AppImages)

### Clone & Build

```bash
git clone --branch main --single-branch https://github.com/f-io/livi.git \
  && cd livi \
  && npm run install:clean \
  && npm run build \
  && npm run build:armLinux
```

## Android Auto

> **Provisioning not supported.** This app does **not** perform the Android Auto first-time provisioning/pairing flow.
> Your phone must already be paired/enrolled **on the dongle**.

**How to provision AA on the dongle:**

1. Use the dongle with a regular head unit **or** the vendor’s mobile app to add your phone once.
2. After the dongle knows your phone, connect the dongle to LIVI — it will attach without running provisioning again.

## Images

<p align="center">
  <img src="docs/images/carplay.png" alt="CarPlay" width="58%" />
</p>
<p align="center">
  <img src="docs/images/carplay_no_phone.png" alt="No Phone" width="48%" align="top" />
  &emsp;
  <img src="docs/images/media.png" alt="Media" width="48%" align="top" />
</p>
<p align="center">
  <img src="docs/images/fft.png" alt="Info" width="48%" align="top" />
  &emsp;
  <img src="docs/images/settings.png" alt="Settings" width="48%" align="top" />
</p>

## Credits

See [CREDITS](CREDITS.md) for acknowledgements and prior art.

## Disclaimer

_Apple and CarPlay are trademarks of Apple Inc. Android and Android Auto are trademarks of Google LLC. This project is not affiliated with or endorsed by Apple or Google. All product names, logos, and brands are the property of their respective owners._

## License

This project is licensed under the MIT License.
