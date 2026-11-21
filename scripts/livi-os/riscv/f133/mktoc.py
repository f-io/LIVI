#!/usr/bin/env python3
# Builds the sunxi-package (toc1) boot0 reads from NOR 0x10000. Layout and checksum match the Melis
# package in the original dump. Arguments: name@run_addr=file, boot0 puts each item at its run_addr.
import struct
import sys

out, items = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]
items = [(name.split("@")[0], int(name.split("@")[1], 0), path) for name, path in items]
HEAD, ITEM = 0x40, 0x170
blobs = [open(path, "rb").read() for _, _, path in items]

off = -(-(HEAD + ITEM * len(items)) // 0x400) * 0x400
toc = bytearray()
for (name, run, _), blob in zip(items, blobs):
    item = bytearray(ITEM)
    item[:len(name)] = name.encode()
    struct.pack_into("<IIIII", item, 0x40, off, len(blob), 0, 3, run)
    item[-4:] = b"IIE;"
    toc += item
    off = -(-(off + len(blob)) // 0x400) * 0x400
size = -(-off // 0x4000) * 0x4000

img = bytearray(size)
head = bytearray(HEAD)
head[:13] = b"sunxi-package"
struct.pack_into("<IIIIII", head, 0x10, 0x89119800, 0x5F0A6C39, 0, 0, len(items), size)
head[-4:] = b"MIE;"
img[:HEAD] = head
img[HEAD:HEAD + len(toc)] = toc
for i, blob in enumerate(blobs):
    o = struct.unpack_from("<I", img, HEAD + i * ITEM + 0x40)[0]
    img[o:o + len(blob)] = blob
struct.pack_into("<I", img, 0x14, sum(struct.unpack("<%dI" % (size // 4), img)) & 0xFFFFFFFF)
open(out, "wb").write(img)
