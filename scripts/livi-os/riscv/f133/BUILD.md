# LIVI OS for the F133-B head unit

Builds boot0, the boot package (OpenSBI, dtb, kernel) and the rootfs for the XFH F133-B head unit
(452×1280 DSI panel, AIC8800D40L, AB5301B). Everything lands in `$TOP/out`, `$TOP` defaults to
`${XDG_CACHE_HOME:-~/.cache}/livi/f133-kernel`.

## Host

Fedora packages:

```bash
sudo dnf install -y gcc-riscv64-linux-gnu binutils-riscv64-linux-gnu make flex bison bc \
  openssl-devel elfutils-libelf-devel dtc lz4-libs squashfs-tools \
  perl-IPC-Cmd perl-Time-Piece
```

The Perl modules are for the OpenSSL that `livi-cp` builds from source on musl.

Rust from `rust-toolchain.toml`, plus the target:

```bash
rustup target add riscv64gc-unknown-linux-musl
```

[xfel](https://github.com/xboot/xfel) for FEL recovery.

## Build

From this directory:

```bash
../../../firmware/build-userspace.sh .
```

```bash
./build.sh
```

```bash
../../../firmware/build-rootfs.sh .
```

`build.sh` also builds GStreamer (`gst.sh`, static, with `gst-patches/`) and `livi`
(`native/crates/livi-os`): UI, core, helper, output and livid as one program, linked under each name in
`/usr/bin`.

## Run

The LIVI OS rcS (`scripts/livi-os/rootfs`) starts LIVI at boot after `board_start` in
`rootfs/etc/livi/board.sh`, its log is `/tmp/livi/core.log`. Settings and pairings are kept on the `data` partition (jffs2).

## Flash

| mtd | image                    |                                           |
| --- | ------------------------ | ----------------------------------------- |
| 0   | `livi-os-f133-boot0.bin` | boot0                                     |
| 1   | `livi-os-f133-mtd1.bin`  | OpenSBI, dtb, kernel                      |
| 2   | `livi-os-f133-mtd2.bin`  | rootfs                                    |
| 3   |                          | data, jffs2 formats it on the first mount |

`../../../firmware/pack-bundle.sh .` packs mtd1 and mtd2 into `livi-os-f133.lfwb` for the
unit's web page.

From a running system, with the image in `/tmp`:

```bash
flashcp /tmp/livi-os-f133-mtd1.bin /dev/mtd1 && reboot
```

From FEL (a unit that does not boot), `fel-boot.sh` writes mtd0 and mtd1, the rootfs follows from the
running system. A running system reboots into FEL with

```bash
devmem 0x07090108 32 0x5AA5A55A; reboot -f
```
