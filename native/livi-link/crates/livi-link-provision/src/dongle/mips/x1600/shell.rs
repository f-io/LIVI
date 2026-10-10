//! Patches the vendor OTA's squashfs rootfs so the dongle opens our bind-shell on 2323 at boot —
//! the same bind-shell the other dongles use, cross-compiled for this one's mipsel/hard-float/
//! o32 ABI (confirmed against the stock `/bin/busybox`'s `.MIPS.abiflags`; see assets/bindshell).
//! No vendor script is edited: a new, standalone S-script is added the way `S61factory_test`
//! already does it on this rootfs, so there is nothing to un-hook if this never ran.

use std::io::Cursor;

use backhand::{FilesystemReader, FilesystemWriter, NodeHeader};

use super::image;

const SHELL: &[u8] = include_bytes!("../../../../assets/bindshell/mipsel");
const SHELL_PATH: &str = "/usr/bin/livi-link-shell";
const INIT_SCRIPT_PATH: &str = "/etc/init.d/S98livi_link_shell";

fn init_script() -> String {
    format!(
        "#!/bin/sh\n\
         \n\
         start() {{\n\
         \t{SHELL_PATH} 2323 &\n\
         }}\n\
         \n\
         stop() {{\n\
         \tkillall livi-link-shell\n\
         }}\n\
         \n\
         case \"$1\" in\n\
         \tstart)\n\
         \t\tstart\n\
         \t\t;;\n\
         \tstop)\n\
         \t\tstop\n\
         \t\t;;\n\
         \trestart|reload)\n\
         \t\tstop\n\
         \t\tstart\n\
         \t\t;;\n\
         \t*)\n\
         \t\techo \"Usage: $0 {{start|stop|restart}}\"\n\
         \t\texit 1\n\
         esac\n\
         \n\
         exit $?\n"
    )
}

fn put(writer: &mut FilesystemWriter, path: &str, data: Vec<u8>) -> Result<(), String> {
    let header = NodeHeader { permissions: 0o755, uid: 0, gid: 0, mtime: 0 };
    let bytes = Cursor::new(data);
    if writer.mut_file(path).is_some() {
        writer.replace_file(path, bytes)
    } else {
        writer.push_file(bytes, path, header)
    }
    .map_err(|e| format!("{path}: {e}"))
}

/// Adds the bind-shell and its boot script to the rootfs, then rebuilds the OTA image around it.
/// Safe to run on an image this already ran on: both files are just replaced in place.
pub fn with_shell(ota: &[u8]) -> Result<Vec<u8>, String> {
    let parts = image::parse(ota)?;
    let fs = FilesystemReader::from_reader(Cursor::new(parts.rootfs))
        .map_err(|e| format!("rootfs: {e}"))?;
    let mut writer = FilesystemWriter::from_fs_reader(&fs).map_err(|e| format!("rootfs: {e}"))?;

    put(&mut writer, SHELL_PATH, SHELL.to_vec())?;
    put(&mut writer, INIT_SCRIPT_PATH, init_script().into_bytes())?;

    let mut squashfs = Cursor::new(Vec::new());
    writer.write(&mut squashfs).map_err(|e| format!("rootfs: {e}"))?;
    Ok(image::build(parts.kernel, &squashfs.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use backhand::InnerNode;
    use std::io::Read;

    fn file_in(rootfs: &[u8], path: &str) -> Option<Vec<u8>> {
        let fs = FilesystemReader::from_reader(Cursor::new(rootfs)).ok()?;
        let node = fs.files().find(|n| n.fullpath.to_string_lossy() == path)?;
        let InnerNode::File(file) = &node.inner else { return None };
        let mut data = Vec::new();
        fs.file(file).reader().read_to_end(&mut data).ok()?;
        Some(data)
    }

    fn fake_rootfs(extra: Option<(&str, Vec<u8>)>) -> Vec<u8> {
        let header = NodeHeader { permissions: 0o755, uid: 0, gid: 0, mtime: 0 };
        let mut fs = FilesystemWriter::default();
        fs.push_dir_all("/usr/bin", header).unwrap();
        fs.push_dir_all("/etc/init.d", header).unwrap();
        if let Some((path, data)) = extra {
            fs.push_file(Cursor::new(data), path, header).unwrap();
        }
        let mut out = Cursor::new(Vec::new());
        fs.write(&mut out).unwrap();
        out.into_inner()
    }

    #[test]
    fn the_shell_and_its_boot_script_land_in_the_rootfs() {
        let ota = image::build(b"kernel-bytes", &fake_rootfs(None));
        let patched = with_shell(&ota).unwrap();
        let parts = image::parse(&patched).unwrap();
        assert_eq!(parts.kernel, b"kernel-bytes");
        assert_eq!(file_in(parts.rootfs, SHELL_PATH).as_deref(), Some(SHELL));
        let script = String::from_utf8(file_in(parts.rootfs, INIT_SCRIPT_PATH).unwrap()).unwrap();
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(&format!("{SHELL_PATH} 2323 &")));
    }

    #[test]
    fn patching_twice_replaces_in_place_instead_of_erroring() {
        let ota = image::build(b"kernel-bytes", &fake_rootfs(None));
        let once = with_shell(&ota).unwrap();
        let twice = with_shell(&once).unwrap();
        let parts = image::parse(&twice).unwrap();
        assert_eq!(file_in(parts.rootfs, SHELL_PATH).as_deref(), Some(SHELL));
    }

    #[test]
    fn an_old_copy_of_the_shell_is_replaced_not_duplicated() {
        let ota = image::build(b"kernel-bytes", &fake_rootfs(Some((SHELL_PATH, vec![0u8; 4]))));
        let patched = with_shell(&ota).unwrap();
        let parts = image::parse(&patched).unwrap();
        assert_eq!(file_in(parts.rootfs, SHELL_PATH).as_deref(), Some(SHELL));
    }

    #[test]
    fn the_shell_is_the_mips_binary_this_dongle_needs() {
        assert_eq!(&SHELL[..4], b"\x7fELF");
        assert_eq!(SHELL[4], 1, "32-bit");
        assert_eq!(SHELL[5], 1, "little-endian");
        let machine = u16::from_le_bytes([SHELL[18], SHELL[19]]);
        assert_eq!(machine, 8, "EM_MIPS");
    }
}
