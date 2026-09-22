#!/usr/bin/env python3
# Packs a raw kernel image into this device's uImage variant: same legacy
# U-Boot header layout, but muboot reads every multi-byte field little-endian
# (confirmed against the real mtd3_boot.img header: magic bytes 56 19 05 27 ==
# 0x27051956 read LE, vs. the spec's big-endian). Stock mkimage always emits
# big-endian headers, so it can't be used here.
import struct, sys, time, zlib

IH_MAGIC = 0x27051956
IH_OS_LINUX = 5
IH_ARCH_ARM = 2
IH_TYPE_KERNEL = 2
IH_COMP_NONE = 0

def build(data, load, ep, name):
    name_b = name.encode()[:32].ljust(32, b'\0')
    dcrc = zlib.crc32(data) & 0xffffffff
    header_no_hcrc = struct.pack('<IIIIIIIBBBB32s',
        IH_MAGIC, 0, int(time.time()), len(data), load, ep, dcrc,
        IH_OS_LINUX, IH_ARCH_ARM, IH_TYPE_KERNEL, IH_COMP_NONE, name_b)
    hcrc = zlib.crc32(header_no_hcrc) & 0xffffffff
    header = struct.pack('<IIIIIIIBBBB32s',
        IH_MAGIC, hcrc, int(time.time()), len(data), load, ep, dcrc,
        IH_OS_LINUX, IH_ARCH_ARM, IH_TYPE_KERNEL, IH_COMP_NONE, name_b)
    assert len(header) == 64
    return header + data

if __name__ == '__main__':
    src, dst, name = sys.argv[1], sys.argv[2], sys.argv[3]
    with open(src, 'rb') as f:
        data = f.read()
    img = build(data, 0x10008000, 0x10008000, name)
    with open(dst, 'wb') as f:
        f.write(img)
    print(f'{dst}: {len(img)} bytes (header 64 + payload {len(data)})')
