# `pi-splash`

Raspberry Pi boot splash support for `LIVI`, built around an animated `plymouth` theme.

## Structure

- `install.sh`
  Main entrypoint. Installs `plymouth`, `ffmpeg`, the LIVI theme scaffold, and the root helper that rebuilds the boot animation.
- `assets/images/`
  Static image assets used as fallback when no animation is selected.
- `assets/videos/`
  Brand-specific `.h264` source videos.
- `installers/`
  System-level helpers used by `install.sh`.
- `lib/`
  Shared shell helpers for splash ids, config access, and video-pair validation.
- `tools/`
  Operator-facing helper scripts such as splash selection.

## Typical flow

1. Install the boot splash stack:

```bash
sudo scripts/install/pi-splash/install.sh
```

2. Choose the splash video:

```bash
scripts/install/pi-splash/tools/set-splashscreen.sh "Range Rover"
```

The installer copies splash assets into `/usr/local/share/livi/pi-splash/assets/`
and installs the shared helper library into `/usr/local/lib/livi/pi-splash/`.
The selector stores the selected splash id in `~/.config/LIVI/config.json` and then
runs the installed root helper, which reads that id, converts the matching videos into
animation frames, and rebuilds the `plymouth` initramfs.

`~/.config/LIVI/config.json` is the source of truth for the selected splash via
`bootSplashId`. `selection.txt` is kept only as a legacy/diagnostic fallback.

Video behavior:

- `brand1.h264` plays once as the intro animation.
- `brand2.h264` starts immediately after that and loops until boot is complete.

3. Reboot the Raspberry Pi:

```bash
sudo reboot
```

## Roll back to the standard Raspberry Pi boot flow

```bash
sudo scripts/install/pi-splash/uninstal.sh
sudo reboot
```

`uninstal.sh` removes the LIVI Plymouth theme and helper, restores the `cmdline.txt`
backup when available, reverts the `config.txt` splash changes, and removes the installed
per-user splash selection.

## What happens during boot

1. `plymouth` suppresses the standard Raspberry Pi splash from the earliest boot stage.
2. The selected splash id is read from `~/.config/LIVI/config.json`, then the matching `.h264` files from `/usr/local/share/livi/pi-splash/assets/videos/` are converted into PNG frames and written into the LIVI `plymouth` theme.
3. On boot, `plymouth` plays `*1.h264` once, then loops `*2.h264` until the graphical session takes over.

## Key files

- Main installer: [install.sh](scripts/install/pi-splash/install.sh)
- Rollback script: [uninstal.sh](scripts/install/pi-splash/uninstal.sh)
- Plymouth animation helper: [apply-plymouth-splash.sh](scripts/install/pi-splash/installers/apply-plymouth-splash.sh)
- Shared shell helpers: [pi-splash-common.sh](scripts/install/pi-splash/lib/pi-splash-common.sh)
- Splash selector: [set-splashscreen.sh](scripts/install/pi-splash/tools/set-splashscreen.sh)
