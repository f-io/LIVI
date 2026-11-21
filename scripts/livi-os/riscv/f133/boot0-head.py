#!/usr/bin/env python3
# Puts the board's private head into a built smaeul sun20i_d1_spl boot0 and fixes its eGON checksum.
# DRAM, uart and SPI pins are those of the shipped Melis boot0: DDR2 at 528 MHz, uart0 on PE2/PE3, SPI NOR
# on PC2-PC7. The EN25QH128 is read with Dual Output (3Bh) at 100 MHz, its SFDP has no 1-1-4.
import struct, sys

DRAM = [0x210, 0x2, 0x7b7bf9, 0x0, 0xd2, 0x0, 0xe73, 0x2, 0x0, 0x0, 0x471992, 0x131a10c, 0x57041,
        0xb4787896, 0x0, 0x48484848, 0x48, 0x1621121e, 0x0, 0x0, 0x0, 0x30010, 0x35, 0x34000000] + [0] * 8
# normal_gpio_cfg: port (1 = PA), pin, function, pull, drive, data, 2 reserved.
UART = [(5, 2, 6, 1, 0xff, 0xff), (5, 3, 6, 1, 0xff, 0xff)]
SPI = [(3, 2, 2, 0), (3, 3, 2, 1), (3, 4, 2, 0), (3, 5, 2, 0), (3, 6, 2, 0), (3, 7, 2, 0)]
# boot_spinor_info_t: readcmd, read_mode, write_mode, flash_size (MiB), erase_size (KiB), delay_cycle,
# lock_flag, frequency.
SPINOR = (0x3b, 0, 0, 16, 64, 0, 0, 100000000)

p = sys.argv[1]
b = bytearray(open(p, "rb").read())
assert b[4:12] == b"eGON.BT0"
b[0x34] = 8
struct.pack_into("<32I", b, 0x38, *DRAM)
struct.pack_into("<i", b, 0xb8, 0)
for i, g in enumerate(UART):
    struct.pack_into("<6Bxx", b, 0xbc + 8 * i, *g)
b[0xf8:0x1f8] = bytes(0x100)
for i, (port, pin, fn, pull) in enumerate(SPI):
    struct.pack_into("<6Bxx", b, 0xf8 + 8 * i, port, pin, fn, pull, 2, 0xff)
b[0x1f8:0x2f8] = bytes(0x100)
struct.pack_into("<8i", b, 0x1f8, *SPINOR)
length = struct.unpack_from("<I", b, 0x10)[0]
struct.pack_into("<I", b, 0x0c, 0x5F0A6C39)
struct.pack_into("<I", b, 0x0c, sum(struct.unpack_from("<%dI" % (length // 4), b)) & 0xFFFFFFFF)
open(p, "wb").write(b)
