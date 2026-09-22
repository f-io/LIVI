//! Wrap a RISC-V Linux Image + DTB into an ANDROID! bootimg that Stock
//! LY_U-Boot 2018.07 (with OpenSBI 1.0 embedded) will boot on a V821B LIVI-Link.
//!
//! The header carries Allwinner's extended DTB descriptor at offsets
//! 1644/1648/1652 (tag=0x67c, len, load=0x80900000). The kernel is gzipped
//! and the whole payload has to fit into Stock's mtd1 slot (0x310000 B).

use std::{env, fs, io::Write, path::Path, process::ExitCode};

use flate2::{Compression, write::GzEncoder};

const PART_SIZE: usize = 0x310000;
const PAGE_SIZE: usize = 0x800;

const KERNEL_ADDR: u32 = 0x8000_0000;
const RAMDISK_ADDR: u32 = 0x8100_0000;
const SECOND_ADDR: u32 = 0x80f0_0000;
const TAGS_ADDR: u32 = 0x8000_0100;
const AW_DTB_TAG: u32 = 0x0000_067c;
const AW_DTB_LOAD: u32 = 0x8090_0000;

const BOARD: &[u8; 16] = b"sun300i_riscv32\0";

fn die(msg: impl AsRef<str>) -> ExitCode {
    eprintln!("mkbootimg-v821b: {}", msg.as_ref());
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: mkbootimg-v821b <Image> <DTB> <out.bin>");
        return ExitCode::from(2);
    }
    let (image, dtb_path, out) = (&args[1], &args[2], &args[3]);

    let image_bytes = match fs::read(image) {
        Ok(b) => b,
        Err(e) => return die(format!("read {image}: {e}")),
    };
    if image_bytes.len() < 56 || &image_bytes[48..56] != b"RISCV\0\0\0" {
        return die("Image is not a RISC-V kernel (missing magic at offset 48)");
    }

    let dtb = match fs::read(dtb_path) {
        Ok(b) => b,
        Err(e) => return die(format!("read {dtb_path}: {e}")),
    };
    if dtb.len() < 4 || dtb[..4] != [0xd0, 0x0d, 0xfe, 0xed] {
        return die("DTB has no FDT magic");
    }

    // Deterministic gzip: level 9, no filename, no timestamp.
    let kernel = {
        let mut e = GzEncoder::new(Vec::new(), Compression::best());
        if let Err(e) = e.write_all(&image_bytes) {
            return die(format!("gzip: {e}"));
        }
        match e.finish() {
            Ok(v) => v,
            Err(e) => return die(format!("gzip finish: {e}")),
        }
    };

    let mut hdr = [0u8; PAGE_SIZE];
    hdr[0..8].copy_from_slice(b"ANDROID!");
    write_u32(&mut hdr, 8, kernel.len() as u32);
    write_u32(&mut hdr, 12, KERNEL_ADDR);
    write_u32(&mut hdr, 16, 0);
    write_u32(&mut hdr, 20, RAMDISK_ADDR);
    write_u32(&mut hdr, 24, 0);
    write_u32(&mut hdr, 28, SECOND_ADDR);
    write_u32(&mut hdr, 32, TAGS_ADDR);
    write_u32(&mut hdr, 36, PAGE_SIZE as u32);
    write_u32(&mut hdr, 40, 2);
    write_u32(&mut hdr, 44, 0);
    hdr[48..64].copy_from_slice(BOARD);
    write_u32(&mut hdr, 1644, AW_DTB_TAG);
    write_u32(&mut hdr, 1648, dtb.len() as u32);
    write_u32(&mut hdr, 1652, AW_DTB_LOAD);

    let mut buf = Vec::with_capacity(PART_SIZE);
    buf.extend_from_slice(&hdr);
    buf.extend_from_slice(&kernel);
    pad_to(&mut buf, PAGE_SIZE);
    buf.extend_from_slice(&dtb);

    println!(
        "kernel gz: {} B  DTB: {} B  total: {} B (0x{:x})",
        kernel.len(),
        dtb.len(),
        buf.len(),
        buf.len()
    );
    if buf.len() > PART_SIZE {
        return die(format!(
            "bootimg is {} B larger than the mtd1 slot ({:#x})",
            buf.len() - PART_SIZE,
            PART_SIZE
        ));
    }
    buf.resize(PART_SIZE, 0);

    if let Some(parent) = Path::new(out).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return die(format!("mkdir {}: {e}", parent.display()));
    }
    if let Err(e) = fs::write(out, &buf) {
        return die(format!("write {out}: {e}"));
    }
    println!("wrote {out}: {} B", buf.len());
    ExitCode::SUCCESS
}

fn write_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

fn pad_to(buf: &mut Vec<u8>, block: usize) {
    let r = buf.len() % block;
    if r != 0 {
        buf.resize(buf.len() + (block - r), 0);
    }
}
