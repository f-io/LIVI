# LIVI Link

A CarPlay dongle, reflashed into a network accessory for LIVI's native CarPlay stack. It is
supported on Linux and macOS, and can provide:

- **MFi authentication** over the network
- **A Wi-Fi access point**
- **Bluetooth** as a vhci on Linux and on macOS the dongle pairs the phone

Each one is enabled separately in the settings. While the dongle is not selected in the settings,
LIVI turns its access point temporarly off to keep interference low.

## Supported hardware

Several bridges are the same board sold under different names. The same product name can also cover different hardware. A row counts as **confirmed** only once someone has sucessfully installed LIVI Link on that product.

| Firmware | Hardware | Sold as | Wi-Fi | State |
| --- | --- | --- | --- | --- |
| `cpc200-ccpa` | NXP i.MX6UL, IW416 | CPC200-CCPA | 5 GHz, 40 MHz | confirmed |
| `cpc200-ccpa` | NXP i.MX6UL, IW416 | CPC200-C2Air | 5 GHz, 40 MHz | not confirmed |
| none | NXP i.MX6UL, IW416  | CPC200-2Air | | not supported |
| `v821b_aic8800d80` | Allwinner V821B, AIC8800D80 | Mini Ultra3 | 5 GHz, 80 MHz | confirmed |
| `v821b_aic8800d80` | Allwinner V821B, AIC8800D80 | CPC200-C2Air | 5 GHz, 80 MHz | not confirmed |
| none | Axera AX520CE, AIC8800D80 | CPC200-C2Air | | not supported |

The provisioning tool probes the dongle before it writes anything, and reports hardware it does not know as not found.

Two reports say the install fails on a CPC200-CCPA running stock firmware `2025.10.15.1127`,
where the dongle drops off USB before anything is written, but works after a downgrade to
`2025.02.25.1521` ([#341](https://github.com/f-io/LIVI/issues/341),
[#347](https://github.com/f-io/LIVI/issues/347)). We could not reproduce it here. A dongle on the
latest stock firmware installed fine. If yours drops off USB during the install, a downgrade and try again.

## Setup

Flashing a dongle is at your own risk. If it goes wrong, open an [issue](https://github.com/f-io/LIVI/issues). Most dongles can be recovered even after a failed flash.

Download `livi-link-provision` for your platform from the release page, then with the dongle
plugged in (with some dongles you also need to be on their Wi-Fi):

```bash
chmod +x livi-link-provision
./livi-link-provision
```

macOS quarantines downloads, so run this first:

```bash
xattr -d com.apple.quarantine livi-link-provision
```

If the dongle is still on stock firmware, the tool asks you to unplug and replug it once. After
that it runs on its own: backup, install, reboot, verify. It takes the backup before it changes
anything and puts it in `~/Library/Application Support/LIVI/dongle-backup/` on macOS, or
`~/.local/share/LIVI/dongle-backup/` on Linux.


## Web interface

<http://livi-link.local/>, or <http://10.10.10.1/> over USB. It shows which firmware the dongle
runs, what the radio is actually doing (channel, width, clients, link rate), Bluetooth, and, if
the LED supports colours, its colour and brightness.

<p align="center">
  <img src="docs/media/livi-link/LL.png" width="600" alt="LIVI Link web interface" />
</p>

## Updating

Under **Firmware**, **Check** looks for a newer version and **Update** installs it. With
**Nightly** on it checks the nightly builds instead of the latest release. The device you have the
web interface open on needs internet access. While the dongle writes, the LED alternates red and
blue. Do not unplug it until that stops.

Every release has the firmware files attached, so you can also upload one by hand.

## LED

If the dongle has an LED, Wi-Fi uses the status LED[^led] and Bluetooth is blue.

| State | LED |
| --- | --- |
| Waiting for a Wi-Fi client | status LED blinks |
| Wi-Fi client connected | status LED on |
| Bluetooth paging | blue blinks |
| Bluetooth connected | blue on |
| Writing firmware | red and blue alternate |

[^led]: Red, or cyan if the LED supports colours. You can change colour and brightness on the
    web interface.

## Getting back to stock

Upload the backup the install made, on the web interface under **Firmware**. The dongle writes it
and reboots into its original firmware. The LED alternates red and blue while it writes. Do not
unplug it until that stops.

## If something goes wrong

If the dongle does not come up on USB or Wi-Fi, give it 30 seconds, then replug it. The logs are under `/tmp` on the dongle.

## Firmware

The firmware carries LIVI's version and the commit it was built from, for example
`9.0.0 (7850de95)`. The web interface shows both under **Firmware**.
