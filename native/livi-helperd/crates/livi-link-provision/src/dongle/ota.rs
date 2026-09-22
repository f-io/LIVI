//! The vendor's own update package for a dongle project, patched so the dongle opens our bind-shell
//! once it has flashed it. The dongle's stock web updater takes the package and does the rest, all
//! we hand it is a package it accepts: the project's own, with our hook in the `customer` partition.
//!
//! Nothing of the vendor's is shipped with the tool, the package comes from the vendor's update
//! server, the same file the dongle's update page fetches, and is kept on disk for the next run.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use backhand::{FilesystemReader, FilesystemWriter, InnerNode, NodeHeader};

use crate::dongle::arm::imx6ul::payload::md5_hex;

/// A file that stands in for the download, for a machine that never sees the vendor's server.
pub const OTA_FILE_ENV: &str = "LIVI_LINK_OTA";

/// The update package is masked with this, byte `i` with `KEY[i % 256]`.
const KEY_HEX: &str = concat!(
    "1269544837f0d56e927e26c3e525c5ee2a968b51ab10f2a9dc1d1b54b2a0a4b5",
    "faf8fd31e9d2a07b50c63e36eb03246613bfb73fc0a9e89cc704f079a4952e9e",
    "7e3bd077fd70f24e3630842133a887c77742e40df6f32c895249915fbe9164e0",
    "af2d03c12bd809974759061c808fbd48b1a155c8958151e7cbe2478974ab6a13",
    "696dd40445de9b8d37a2a9b73167ff02375e0617e8eeb4aff2a42897395735c7",
    "62053e807530ceab6052cd94fa545b513361697b4f1d2a42c152d9fba90ec22c",
    "2401ac89317a3592cc0226c6568117e94ca0ba4346b40f2e925f1d080d750005",
    "620dc92322fdd4486c6f4f6fd52788a108a5ca9dd61b85a1fecc2b7277958550"
);

const ENVELOPE_TAIL: &[u8] = b"  update.swu\n";
const CPIO_HEADER: usize = 110;
const CPIO_CRC_MAGIC: &[u8; 6] = b"070702";
const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

const SCRIPT_PATH: &str = "/app/lyLink.sh";
const SHELL_PATH: &str = "/app/livi-link-shell";
const HOOK_MARKER: &str = "# LIVI-LINK-SHELL-HOOK:";

/// Started from the vendor's own boot script, after it has settled: copies the shell out of the
/// read-only partition and runs it on 2323. What it saw of the machine goes to the dongle's web
/// root, so a dongle where the shell does not come up can still be looked at over HTTP.
const SHELL_HOOK: &str = "
# LIVI-LINK-SHELL-HOOK: bind shell on 2323 for the LIVI Link provisioning tool.
if [ ! -f /tmp/.livi_link_shell_started ]; then
\ttouch /tmp/.livi_link_shell_started
\t(
\t\tsleep 3
\t\tmkdir -p /tmp/ota/www
\t\tcp $WORKDIR/livi-link-shell /tmp/livi-link-shell
\t\tchmod 755 /tmp/livi-link-shell
\t\t{
\t\t\techo \"=== uname -a ===\"
\t\t\tuname -a
\t\t\techo \"=== /proc/cpuinfo ===\"
\t\t\tcat /proc/cpuinfo
\t\t\techo \"=== /proc/mtd ===\"
\t\t\tcat /proc/mtd
\t\t\techo \"=== starting ===\"
\t\t} > /tmp/ota/www/livi-link-shell.txt 2>&1
\t\t/tmp/livi-link-shell 2323 2>>/tmp/ota/www/livi-link-shell.txt
\t\techo \"livi-link-shell exited rc=$?\" >> /tmp/ota/www/livi-link-shell.txt
\t) &
fi
";

const SHELL_ARM: &[u8] = include_bytes!("../../assets/bindshell/armv7");
const SHELL_RISCV32: &[u8] = include_bytes!("../../assets/bindshell/riscv32");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Arm,
    Riscv32,
}

impl Arch {
    pub fn name(self) -> &'static str {
        match self {
            Arch::Arm => "ARMv7",
            Arch::Riscv32 => "RISC-V 32",
        }
    }

    /// The bind-shell built for this architecture.
    pub fn shell(self) -> &'static [u8] {
        match self {
            Arch::Arm => SHELL_ARM,
            Arch::Riscv32 => SHELL_RISCV32,
        }
    }
}

fn key() -> &'static [u8; 256] {
    static KEY: OnceLock<[u8; 256]> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut key = [0u8; 256];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&KEY_HEX[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
        key
    })
}

fn mask(data: &[u8]) -> Vec<u8> {
    let key = key();
    data.iter().enumerate().map(|(i, b)| b ^ key[i & 0xff]).collect()
}

// ---------------------------------------------------------------------------------------------
// The archive inside: cpio with a text header ("newc"), members sw-description, kernel, customer.
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct Entry {
    magic: [u8; 6],
    fields: [u32; 13],
    name: String,
    data: Vec<u8>,
}

const fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn read_entries(archive: &[u8]) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut at = 0;
    loop {
        let header = archive
            .get(at..at + CPIO_HEADER)
            .ok_or("the update ends in the middle of a header")?;
        let magic: [u8; 6] = header[..6].try_into().map_err(|_| "short header")?;
        if !magic.starts_with(b"07070") {
            return Err("the update is not a cpio archive".into());
        }
        let mut fields = [0u32; 13];
        for (i, field) in fields.iter_mut().enumerate() {
            let text = std::str::from_utf8(&header[6 + i * 8..14 + i * 8]).map_err(|e| e.to_string())?;
            *field = u32::from_str_radix(text, 16).map_err(|e| format!("cpio header field {i}: {e}"))?;
        }
        let (file_size, name_size) = (fields[6] as usize, fields[11] as usize);
        let name_bytes = archive
            .get(at + CPIO_HEADER..at + CPIO_HEADER + name_size)
            .ok_or("the update ends in the middle of a name")?;
        let name = String::from_utf8_lossy(name_bytes.strip_suffix(&[0]).unwrap_or(name_bytes)).into_owned();
        let data_at = align4(at + CPIO_HEADER + name_size);
        let data = archive
            .get(data_at..data_at + file_size)
            .ok_or_else(|| format!("the update ends in the middle of {name}"))?
            .to_vec();
        let last = name == "TRAILER!!!";
        entries.push(Entry { magic, fields, name, data });
        if last {
            return Ok(entries);
        }
        at = align4(data_at + file_size);
    }
}

fn pad4(out: &mut Vec<u8>) {
    out.resize(align4(out.len()), 0);
}

fn write_entries(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        let mut name = entry.name.clone().into_bytes();
        name.push(0);
        let mut fields = entry.fields;
        fields[6] = entry.data.len() as u32;
        fields[11] = name.len() as u32;
        fields[12] = if &entry.magic == CPIO_CRC_MAGIC {
            entry.data.iter().fold(0u32, |sum, b| sum.wrapping_add(u32::from(*b)))
        } else {
            0
        };
        out.extend_from_slice(&entry.magic);
        for field in fields {
            out.extend_from_slice(format!("{field:08X}").as_bytes());
        }
        out.extend_from_slice(&name);
        pad4(&mut out);
        out.extend_from_slice(&entry.data);
        pad4(&mut out);
    }
    out.resize(out.len().next_multiple_of(4096), 0);
    out
}

// ---------------------------------------------------------------------------------------------
// The package.
// ---------------------------------------------------------------------------------------------

pub struct Ota {
    entries: Vec<Entry>,
}

impl Ota {
    pub fn open(image: &[u8]) -> Result<Self, String> {
        let plain = mask(image);
        let tail = plain.get(32..32 + ENVELOPE_TAIL.len());
        if tail != Some(ENVELOPE_TAIL) {
            return Err("this is not an update image of the vendor's".into());
        }
        let body = &plain[32 + ENVELOPE_TAIL.len()..];
        let stated = String::from_utf8_lossy(&plain[..32]).to_ascii_lowercase();
        if stated != md5_hex(body) {
            return Err("the update image is damaged (its checksum does not match)".into());
        }
        Ok(Self { entries: read_entries(body)? })
    }

    fn part(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// The project the package is for, `ly7115`. The dongle's updater turns a package of another
    /// project down.
    pub fn project(&self) -> Option<String> {
        let text = String::from_utf8_lossy(&self.part("sw-description")?.data).into_owned();
        let line = text.lines().find(|l| l.contains("ly_project_name"))?;
        line.split('"').nth(1).map(str::to_string)
    }

    /// The processor the dongle runs, from the programs of its own that the package carries.
    pub fn arch(&self) -> Result<Arch, String> {
        let customer = self.part("customer").ok_or("the update has no customer partition")?;
        let fs = FilesystemReader::from_reader(Cursor::new(&customer.data[..]))
            .map_err(|e| format!("customer partition: {e}"))?;
        for node in fs.files() {
            let InnerNode::File(file) = &node.inner else {
                continue;
            };
            let mut head = [0u8; 20];
            if fs.file(file).reader().read_exact(&mut head).is_err() || &head[..4] != b"\x7fELF" {
                continue;
            }
            return match (head[4], head[5], u16::from_le_bytes([head[18], head[19]])) {
                (1, 1, 40) => Ok(Arch::Arm),
                (1, 1, 243) => Ok(Arch::Riscv32),
                (class, data, machine) => Err(format!(
                    "a processor we have no bind-shell for (ELF class {class}, data {data}, machine {machine})"
                )),
            };
        }
        Err("the customer partition holds no program to tell the processor from".into())
    }

    /// The package with the bind-shell in it, masked and ready for the dongle's updater.
    pub fn with_shell(&self, shell: &[u8]) -> Result<Vec<u8>, String> {
        let customer = self.part("customer").ok_or("the update has no customer partition")?;
        let fs = FilesystemReader::from_reader(Cursor::new(&customer.data[..]))
            .map_err(|e| format!("customer partition: {e}"))?;
        let script = read_text(&fs, SCRIPT_PATH)
            .ok_or_else(|| format!("{SCRIPT_PATH} is not in the customer partition, so there is nothing to hook into"))?;

        let mut writer = FilesystemWriter::from_fs_reader(&fs).map_err(|e| format!("customer partition: {e}"))?;
        writer
            .replace_file(SCRIPT_PATH, Cursor::new(hooked(&script).into_bytes()))
            .map_err(|e| format!("{SCRIPT_PATH}: {e}"))?;
        let bytes = Cursor::new(shell.to_vec());
        if writer.mut_file(SHELL_PATH).is_some() {
            writer.replace_file(SHELL_PATH, bytes)
        } else {
            writer.push_file(bytes, SHELL_PATH, NodeHeader { permissions: 0o755, uid: 0, gid: 0, mtime: 0 })
        }
        .map_err(|e| format!("{SHELL_PATH}: {e}"))?;
        let mut squashfs = Cursor::new(Vec::new());
        writer.write(&mut squashfs).map_err(|e| format!("customer partition: {e}"))?;

        let mut entries = self.entries.clone();
        if let Some(entry) = entries.iter_mut().find(|e| e.name == "customer") {
            entry.data = squashfs.into_inner();
        }
        let body = write_entries(&entries);
        let mut envelope = md5_hex(&body).into_bytes();
        envelope.extend_from_slice(ENVELOPE_TAIL);
        envelope.extend_from_slice(&body);
        Ok(mask(&envelope))
    }
}

fn read_text(fs: &FilesystemReader<'_>, path: &str) -> Option<String> {
    let node = fs.files().find(|n| n.fullpath.to_string_lossy() == path)?;
    let InnerNode::File(file) = &node.inner else {
        return None;
    };
    let mut text = String::new();
    fs.file(file).reader().read_to_string(&mut text).ok()?;
    Some(text)
}

/// Our hook right behind the shebang, once: an earlier one of ours is taken out first.
fn hooked(script: &str) -> String {
    let script = match script.find(&format!("\n{HOOK_MARKER}")) {
        Some(start) => match script[start..].find("\nfi\n") {
            Some(end) => format!("{}{}", &script[..start], &script[start + end + 4..]),
            None => script.to_string(),
        },
        None => script.to_string(),
    };
    let (shebang, rest) = script.split_once('\n').unwrap_or((&script, ""));
    format!("{shebang}\n{SHELL_HOOK}{rest}")
}

// ---------------------------------------------------------------------------------------------
// Getting the vendor's package.
// ---------------------------------------------------------------------------------------------

/// Where the vendor keeps a project's update: a bucket per market, a folder per project, both
/// read off the `appver` (`26060818.7115.2`, the market is the last field).
fn origin_and_folder(appver: &str) -> Option<(String, String)> {
    let mut fields = appver.split('.');
    let (_date, project, market) = (fields.next()?, fields.next()?, fields.next()?);
    if project.is_empty() || !project.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (bucket, region) = if market == "2" {
        ("cpbox-abroad", "oss-us-west-1")
    } else {
        ("cpbox", "oss-cn-shenzhen")
    };
    Some((format!("https://{bucket}.{region}.aliyuncs.com"), project.to_string()))
}

pub fn version_url(appver: &str) -> Option<String> {
    origin_and_folder(appver).map(|(origin, folder)| format!("{origin}/{folder}/version.json"))
}

/// The address of the current package out of a `version.json`, only ever one on the vendor's own
/// server.
fn image_url(version_json: &str, origin: &str) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(version_json).map_err(|e| format!("version.json: {e}"))?;
    let url = json
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or("version.json names no update")?;
    if !url.starts_with(&format!("{origin}/")) {
        return Err(format!("version.json points to {url}, which is not the vendor's server"));
    }
    Ok(url.to_string())
}

fn get(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .timeout(Duration::from_secs(300))
        .call()
        .map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take(limit)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(body)
}

/// The package for the dongle with this `appver`: the file `LIVI_LINK_OTA` names, else the copy
/// kept from an earlier run, else the download. A computer joined to the dongle's Wi-Fi has no way
/// to the internet, which is the usual reason this fails, so it says what to do about that.
pub fn fetch(appver: &str, cache: &Path) -> Result<Vec<u8>, String> {
    if let Ok(path) = std::env::var(OTA_FILE_ENV) {
        println!("== using the update image {path}");
        return std::fs::read(&path).map_err(|e| format!("{path}: {e}"));
    }
    let (origin, folder) = origin_and_folder(appver).ok_or_else(|| format!("no project in appver {appver:?}"))?;
    let saved: PathBuf = cache.join(format!("ly{folder}-update.img"));
    if let Ok(bytes) = std::fs::read(&saved)
        && Ota::open(&bytes).is_ok()
    {
        println!("== using the saved download {}", crate::tilde(&saved));
        return Ok(bytes);
    }

    let version_url = format!("{origin}/{folder}/version.json");
    println!("== looking up the vendor's update for this dongle: {version_url}");
    let downloaded = get(&version_url, 1 << 20)
        .and_then(|text| image_url(&String::from_utf8_lossy(&text), &origin))
        .and_then(|url| {
            println!("== downloading {url}");
            get(&url, MAX_DOWNLOAD).map(|bytes| (url, bytes))
        });
    match downloaded {
        Ok((_, bytes)) => {
            if let Err(e) = std::fs::create_dir_all(cache).and_then(|()| std::fs::write(&saved, &bytes)) {
                println!("== could not save the download for the next run: {e}");
            }
            println!("== downloaded {} bytes", bytes.len());
            Ok(bytes)
        }
        Err(e) => Err(format!(
            "could not get the vendor's update for this dongle ({e}).\n\
             The dongle's own Wi-Fi has no internet. Connect this computer to the internet as well \
             (cable or hotspot) and run the tool again, it keeps the download. Or open {version_url} \
             on any device, download the update it names, save it as {} and run again.",
            crate::tilde(&saved)
        )),
    }
}

/// The package for `project` (the dongle's), patched for its processor, ready to upload.
pub fn shell_image(appver: &str, project: &str, cache: &Path) -> Result<Vec<u8>, String> {
    let bytes = fetch(appver, cache)?;
    let ota = Ota::open(&bytes)?;
    match ota.project() {
        Some(p) if p == project => {}
        Some(p) => return Err(format!("that update is for project {p}, this dongle is {project}")),
        None => return Err("the update names no project".into()),
    }
    let arch = ota.arch()?;
    println!("== the update carries {} programs, patching it with the matching bind-shell", arch.name());
    ota.with_shell(arch.shell())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package of the vendor's shape made here, so no firmware of theirs is needed for the tests:
    /// an update image with the members `sw-description`, `kernel` and `customer`, the last one a
    /// squashfs with a boot script and one program of the given ELF machine (40 ARM, 243 RISC-V).
    fn fake_package(project: &str, machine: u16) -> Vec<u8> {
        let mut program = vec![0u8; 64];
        program[..4].copy_from_slice(b"\x7fELF");
        program[4] = 1; // 32 bit
        program[5] = 1; // little endian
        program[18..20].copy_from_slice(&machine.to_le_bytes());

        let header = NodeHeader { permissions: 0o755, uid: 0, gid: 0, mtime: 0 };
        let mut fs = FilesystemWriter::default();
        fs.push_dir_all("/app", header).unwrap();
        fs.push_file(Cursor::new(b"#!/bin/sh\necho vendor boot\n".to_vec()), SCRIPT_PATH, header).unwrap();
        fs.push_file(Cursor::new(program), "/app/lylink", header).unwrap();
        let mut squashfs = Cursor::new(Vec::new());
        fs.write(&mut squashfs).unwrap();

        let description = format!("software =\n{{\n    ly_project_name = \"{project}\";\n}}\n");
        let member = |name: &str, data: Vec<u8>| Entry {
            magic: *b"070701",
            fields: [0; 13],
            name: name.into(),
            data,
        };
        let body = write_entries(&[
            member("sw-description", description.into_bytes()),
            member("kernel", vec![0x56, 0x19, 0x05, 0x27, 1, 2, 3, 4]),
            member("customer", squashfs.into_inner()),
            member("TRAILER!!!", Vec::new()),
        ]);
        let mut envelope = md5_hex(&body).into_bytes();
        envelope.extend_from_slice(ENVELOPE_TAIL);
        envelope.extend_from_slice(&body);
        mask(&envelope)
    }

    /// A customer partition's file, out of a package.
    fn file_in(ota: &Ota, path: &str) -> Option<Vec<u8>> {
        let customer = ota.part("customer")?;
        let fs = FilesystemReader::from_reader(Cursor::new(&customer.data[..])).ok()?;
        let node = fs.files().find(|n| n.fullpath.to_string_lossy() == path)?;
        let InnerNode::File(file) = &node.inner else {
            return None;
        };
        let mut data = Vec::new();
        fs.file(file).reader().read_to_end(&mut data).ok()?;
        Some(data)
    }

    #[test]
    fn the_key_is_all_there() {
        assert_eq!(KEY_HEX.len(), 512);
        assert!(KEY_HEX.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn a_package_is_read_for_its_project_and_processor() {
        let arm = Ota::open(&fake_package("ly7115", 40)).unwrap();
        assert_eq!(arm.project().as_deref(), Some("ly7115"));
        assert_eq!(arm.arch(), Ok(Arch::Arm));
        let riscv = Ota::open(&fake_package("ly6238", 243)).unwrap();
        assert_eq!(riscv.project().as_deref(), Some("ly6238"));
        assert_eq!(riscv.arch(), Ok(Arch::Riscv32));
    }

    #[test]
    fn a_processor_we_have_no_bind_shell_for_is_reported() {
        let x86_64 = Ota::open(&fake_package("ly0001", 62)).unwrap();
        assert!(x86_64.arch().is_err());
    }

    #[test]
    fn writing_the_archive_back_changes_nothing() {
        let plain = mask(&fake_package("ly7115", 40));
        let body = &plain[32 + ENVELOPE_TAIL.len()..];
        assert_eq!(write_entries(&read_entries(body).unwrap()), body);
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused() {
        assert!(Ota::open(b"not an update").is_err());
        let mut image = fake_package("ly7115", 40);
        let middle = image.len() / 2;
        image[middle] ^= 1;
        assert!(Ota::open(&image).is_err());
    }

    #[test]
    fn the_bind_shells_are_programs_of_the_processor_they_are_for() {
        let machine = |shell: &[u8]| u16::from_le_bytes([shell[18], shell[19]]);
        assert_eq!(&Arch::Arm.shell()[..4], b"\x7fELF");
        assert_eq!(machine(Arch::Arm.shell()), 40);
        assert_eq!(&Arch::Riscv32.shell()[..4], b"\x7fELF");
        assert_eq!(machine(Arch::Riscv32.shell()), 243);
    }

    #[test]
    fn a_patched_package_is_a_package_again_with_the_hook_once() {
        let base = Ota::open(&fake_package("ly7115", 40)).unwrap();
        let patched = Ota::open(&base.with_shell(Arch::Arm.shell()).unwrap()).unwrap();
        assert_eq!(patched.project().as_deref(), Some("ly7115"));
        assert_eq!(patched.arch(), Ok(Arch::Arm));
        assert_eq!(file_in(&patched, SHELL_PATH).as_deref(), Some(Arch::Arm.shell()));
        let script = String::from_utf8(file_in(&patched, SCRIPT_PATH).unwrap()).unwrap();
        assert_eq!(script.matches(HOOK_MARKER).count(), 1);
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.ends_with("echo vendor boot\n"));
        // The kernel and the vendor's other files are as they were.
        assert_eq!(patched.part("kernel").unwrap().data, base.part("kernel").unwrap().data);
        assert!(file_in(&patched, "/app/lylink").is_some());
    }

    #[test]
    fn patching_a_patched_package_does_not_stack_a_second_hook() {
        let once = Ota::open(&Ota::open(&fake_package("ly7115", 40)).unwrap().with_shell(Arch::Arm.shell()).unwrap()).unwrap();
        let twice = Ota::open(&once.with_shell(Arch::Arm.shell()).unwrap()).unwrap();
        let script = String::from_utf8(file_in(&twice, SCRIPT_PATH).unwrap()).unwrap();
        assert_eq!(script.matches(HOOK_MARKER).count(), 1);
        assert_eq!(file_in(&twice, SHELL_PATH).as_deref(), Some(Arch::Arm.shell()));
    }

    #[test]
    fn the_hook_replaces_itself_instead_of_piling_up() {
        let script = "#!/bin/sh\n# vendor\necho hi\n";
        let once = hooked(script);
        assert_eq!(hooked(&once), once);
        assert!(once.ends_with("# vendor\necho hi\n"));
    }

    #[test]
    fn the_vendor_server_is_read_off_the_appver() {
        assert_eq!(
            version_url("26060818.7115.2").as_deref(),
            Some("https://cpbox-abroad.oss-us-west-1.aliyuncs.com/7115/version.json")
        );
        assert_eq!(
            version_url("26060818.6238.1").as_deref(),
            Some("https://cpbox.oss-cn-shenzhen.aliyuncs.com/6238/version.json")
        );
        assert_eq!(version_url("nope"), None);
        assert_eq!(version_url("1.x.2"), None);
    }

    #[test]
    fn only_an_address_on_the_vendor_server_is_followed() {
        let origin = "https://cpbox-abroad.oss-us-west-1.aliyuncs.com";
        let good = r#"{"appver":"1","url":"https://cpbox-abroad.oss-us-west-1.aliyuncs.com/7115/update.img"}"#;
        assert_eq!(
            image_url(good, origin).as_deref(),
            Ok("https://cpbox-abroad.oss-us-west-1.aliyuncs.com/7115/update.img")
        );
        let elsewhere = r#"{"url":"https://example.com/update.img"}"#;
        assert!(image_url(elsewhere, origin).is_err());
        let lookalike = r#"{"url":"https://cpbox-abroad.oss-us-west-1.aliyuncs.com.evil.example/x"}"#;
        assert!(image_url(lookalike, origin).is_err());
        assert!(image_url("{}", origin).is_err());
    }
}
