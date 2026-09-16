#!/usr/bin/env bash
# Pack V821B firmware images (mtd1 bootimg + mtd3 squashfs) into one .lfwb
# bundle. Format:
#   Header (8 B):    magic "LFWB" | version:u8=1 | count:u8 | reserved:u16
#   Desc (N*12 B):   type:u8 | flags:u8 | reserved:u16 | length:u32 | crc32:u32
#   Payload:         images concatenated in descriptor order (no padding)
#
# Types: 1 = mtd1 (bootimg), 3 = mtd3 (rootfs).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT_DIR=${OUT_DIR:-$HOME/LocalDev/tina-test/out}
BUNDLE="$OUT_DIR/livi-link-v821b.lfwb"
MTD1="$OUT_DIR/livi-link-v821b-mtd1.bin"
MTD3="$OUT_DIR/livi-link-v821b-mtd3.bin"

log(){ printf "\033[1;36m[livi-bundle]\033[0m %s\n" "$*"; }
[[ -f "$MTD1" ]] || { log "missing $MTD1"; exit 1; }
[[ -f "$MTD3" ]] || { log "missing $MTD3"; exit 1; }

python3 - "$MTD1" "$MTD3" "$BUNDLE" <<'PY'
import sys, struct, zlib
mtd1_path, mtd3_path, out_path = sys.argv[1:4]
imgs = []
for typ, path in [(1, mtd1_path), (3, mtd3_path)]:
    with open(path, "rb") as f:
        data = f.read()
    imgs.append((typ, data, zlib.crc32(data) & 0xffffffff))
hdr = b"LFWB" + bytes([1, len(imgs), 0, 0])
descs = b""
for typ, data, crc in imgs:
    descs += struct.pack("<BBHII", typ, 0, 0, len(data), crc)
payload = b"".join(d for _, d, _ in imgs)
with open(out_path, "wb") as f:
    f.write(hdr); f.write(descs); f.write(payload)
total = len(hdr) + len(descs) + len(payload)
print(f"wrote {out_path}: {total} B (header {len(hdr)} + descs {len(descs)} + payload {len(payload)})")
for typ, data, crc in imgs:
    print(f"  type={typ}  len={len(data)}  crc32={crc:08x}")
PY
