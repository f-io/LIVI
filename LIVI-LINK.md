# LIVI Link

A CPC200-CCPA dongle reflashed into a network attached accessory for LIVI's native CarPlay stack.
It works on Linux and on macOS and carries three things, each of them picked separately in the
settings:

- **MFi authentication**, the coprocessor CarPlay needs, reachable over the network
- **A Wi-Fi access point**, selectable as the Wi-Fi interface for wireless sessions
- **Bluetooth**, selectable as the Bluetooth adapter on Linux. On macOS the dongle pairs with the
  phone itself and passes the session on to LIVI

A Mac has no I²C bus to put a coprocessor on, so there this is the only route to CarPlay besides BAA. On Linux
it is an alternative to a chip on the board, and a way to add an access point and a Bluetooth
adapter to a machine that has neither. LIVI does disable the dongles acces point if it is not choosen within the settings to reduce interference.

The iPhone plugs into the host. The dongle's own OTG port works on MacOS only.

## Setup

Download `livi-link-provision` for your platform from the release page, then with the dongle
plugged in:

```bash
chmod +x livi-link-provision
./livi-link-provision
```

macOS quarantines downloads, so there run this first:

```bash
xattr -d com.apple.quarantine livi-link-provision
```

If the dongle is still stock the tool asks you to unplug and replug it once, then runs
on its own: backup, install, reboot, verify. The backup is taken before anything changes, under
`~/Library/Application Support/LIVI/dongle-backup/` (macOS) or `~/.local/share/LIVI/dongle-backup/`
(Linux).

<p align="center">
  <a href="https://f-io.github.io/LIVI/media/livi-link/LL.mp4"><img src="docs/media/livi-link/LL_preview.png" width="600" alt="LIVI Link install demo" /></a>
</p>

## Getting back to stock

Open <http://10.10.10.1/>, pick the backup's `rootfs.img` under **Recovery** and press
**Restore**. The dongle writes it and reboots into its original firmware. The red and blue LED
alternate while it writes, do not unplug until they stop.

<p align="center">
  <img src="docs/media/livi-link/LL.png" width="600" alt="LIVI Link web interface" />
</p>

## If something goes wrong

If the dongle does not come up on USB or Wi-Fi, wait 30 seconds for it to roll back and reboot,
then replug if it stays quiet. Logs live at <http://10.10.10.1/>: `/tmp/livi-link.log`,
`/tmp/l2fwd-watch.log` and `/tmp/flash.log`.

## Firmware

| Version | State |
| --- | --- |
| 0.1.0 | dev |
