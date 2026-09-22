//! What the dongle carries, all of it embedded. The tool is a single download, so the armv7
//! binary CI builds into `assets/livi-link/cpc200-ccpa` is baked in rather than read from disk.

use md5::{Digest, Md5};

/// Where the stack lives on jffs2; `livi-link.sh` unpacks it into /tmp/livi at boot.
pub const LIVI_DIR: &str = "/script/livi";
/// rcS runs this at boot.
pub const BRINGUP_REMOTE: &str = "/script/start_main_service.sh";
/// The vendor init, kept so the web UI can boot it again instead of ours.
pub const STOCK_BACKUP: &str = "/script/start_main_service.sh.orig";
/// The dongle's own web tools, installed persistently. boa copies /etc/boa to /tmp/boa at boot,
/// so a temporary copy would be gone after a restart.
pub const BOA_CGI: &str = "/etc/boa/cgi-bin/server.cgi";
pub const BOA_INDEX: &str = "/etc/boa/www/index.html";
/// Vendor web assets our page replaces.
pub const BOA_LEFTOVERS: &str =
    "/etc/boa/www/js /etc/boa/www/lang /etc/boa/www/static /etc/boa/www/index.html.gz";

/// What is installed, so the tool can tell an old dongle from a current one.
pub const VERSION_FILE: &str = "/script/livi/version";

/// Recognises our own boot script, so it is never mistaken for the stock one.
pub const BRINGUP_MARKER: &str = "LIVI bridge bring-up";

/// The whole relay stack, one binary; `livi-link.sh` links it under each tool name.
pub const BINARY: &str = "cpc200-ccpa";
/// Superseded builds, removed so a dongle carries only what it runs: the per-tool binaries from
/// before the stack became one, and the tunnel that preceded livi-usbproxy.
pub const OBSOLETE: [&str; 7] = [
    "/script/livi/seedrng.gz",
    "/script/livi/mfid.gz",
    "/script/livi/livi-usbproxy.gz",
    "/script/livi/l2fwd.gz",
    "/script/livi/mdnsd.gz",
    "/script/livi/carkit_tunnel.gz",
    "/script/livi/livi-link.gz",
];
/// Ports the stack answers on once it runs: mfid and livi-usbproxy.
pub const STACK_PORTS: [u16; 2] = [5000, 5003];
/// Processes that must be running afterwards.
pub const STACK_PROCESSES: [&str; 5] = ["seedrng", "mfid", "livi-usbproxy", "l2fwd-watch", "mdnsd"];

const BRINGUP: &str = include_str!("../../../bin/livi-link/scripts/livi-bringup.sh");
const LINK: &str = include_str!("../../../bin/livi-link/scripts/livi-link.sh");
const L2FWD_WATCH: &str = include_str!("../../../bin/livi-link/scripts/l2fwd-watch.sh");
const FLASH_IMAGE: &str = include_str!("../../../bin/livi-link/scripts/flash-image.sh");
const STACK: &[u8] = include_bytes!("../../../../../assets/livi-link/cpc200-ccpa/cpc200-ccpa.gz");
const SERVER_CGI: &str = include_str!("../../../bin/livi-link/web/server.cgi");
const INDEX_HTML: &str = include_str!("../../../bin/livi-link/web/index.html");


pub struct File {
    pub remote: String,
    pub data: Vec<u8>,
    pub md5: String,
}

impl File {
    fn new(remote: String, data: Vec<u8>) -> Self {
        let md5 = md5_hex(&data);
        Self { remote, data, md5 }
    }
}

/// Everything the device should carry, the binary first.
pub fn files() -> Vec<File> {
    let mut files = Vec::new();
    files.push(File::new(format!("{LIVI_DIR}/{BINARY}.gz"), STACK.to_vec()));
    files.push(File::new(format!("{LIVI_DIR}/livi-link.sh"), LINK.into()));
    files.push(File::new(format!("{LIVI_DIR}/l2fwd-watch.sh"), L2FWD_WATCH.into()));
    files.push(File::new(format!("{LIVI_DIR}/flash-image.sh"), FLASH_IMAGE.into()));
    files.push(File::new(BRINGUP_REMOTE.to_string(), BRINGUP.into()));
    files.push(File::new(BOA_CGI.to_string(), SERVER_CGI.into()));
    files.push(File::new(BOA_INDEX.to_string(), INDEX_HTML.into()));
    // Last, because it names what the others add up to.
    let digest: String = files.iter().map(|f| f.md5.as_str()).collect();
    files.push(File::new(VERSION_FILE.to_string(), version(&digest).into_bytes()));
    files
}

/// LIVI's release, passed in by CI. A build from a working tree keeps the crate's version.
pub fn release() -> &'static str {
    option_env!("LIVI_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The version and a short digest of everything it installs, so a dongle that carries the same
/// string carries the same files.
pub fn version(digest: &str) -> String {
    format!("{} {}\n", release(), &md5_hex(digest.as_bytes())[..8])
}

/// The release and the digest a version line is made of.
pub fn parts(line: &str) -> (&str, &str) {
    let line = line.trim();
    line.split_once(' ').unwrap_or((line, ""))
}

/// What `files()` writes to `VERSION_FILE`.
pub fn current_version() -> String {
    let digest: String = files()
        .iter()
        .filter(|f| f.remote != VERSION_FILE)
        .map(|f| f.md5.as_str())
        .collect();
    version(&digest)
}

pub fn md5_hex(data: &[u8]) -> String {
    Md5::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_version_line_splits_into_release_and_digest() {
        assert_eq!(super::parts("9.0.0 3f2a1b9c\n"), ("9.0.0", "3f2a1b9c"));
        assert_eq!(super::parts("9.0.0"), ("9.0.0", ""));
    }

    use super::*;

    #[test]
    fn hashes_like_md5sum() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn the_embedded_scripts_are_the_ones_we_ship() {
        assert!(BRINGUP.contains(BRINGUP_MARKER));
        // livi-link.sh unpacks the binary the asset build produces and links every process
        // name the verify step then looks for.
        assert!(LINK.contains("*.gz"), "the launcher finds the stack .gz by glob, not a hard-coded name");
        for name in STACK_PROCESSES {
            let started = name.strip_suffix("-watch").unwrap_or(name);
            assert!(LINK.contains(started), "livi-link.sh does not mention {started}");
        }
        assert!(L2FWD_WATCH.contains("l2fwd"));
        // The page and the script it calls have to agree on where that script lives.
        assert!(INDEX_HTML.contains("flash_image"));
        assert!(SERVER_CGI.contains("/script/livi/flash-image.sh"));
    }

    #[test]
    fn the_flash_script_refuses_before_it_erases() {
        let erase = FLASH_IMAGE.find("flash_erase $dev").expect("no erase");
        for guard in ["rootfs)", "-ne \"$psize\"", "!= \"$want\""] {
            let at = FLASH_IMAGE.find(guard).unwrap_or_else(|| panic!("no guard {guard}"));
            assert!(at < erase, "{guard} must be checked before erasing");
        }
        // Only the rootfs: the kernel and the bootloader are never touched by provisioning, and
        // recovering a broken one needs serial or JTAG.
        assert!(!FLASH_IMAGE.contains("kernel)"));
        assert!(!FLASH_IMAGE.contains("uboot)"));
    }
}
