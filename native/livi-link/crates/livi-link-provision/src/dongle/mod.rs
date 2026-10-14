pub mod arm;
pub mod hook;
pub mod lfwb;
pub mod link;
pub mod mips;
pub mod ota;
pub mod probe;
pub mod rescue;
pub mod riscv;
pub mod shell;
pub mod web;

pub const DONGLE_HOST: &str = "192.168.50.100";
pub const BIND_SHELL_PORT: u16 = 2323;

/// Puts `data` at `path` in a squashfs being rebuilt, replacing it in place if a patch already put
/// something there — so patching twice (or patching an image that already carries our shell)
/// replaces instead of erroring or duplicating.
pub(crate) fn put_file(
    writer: &mut backhand::FilesystemWriter,
    path: &str,
    data: Vec<u8>,
    header: backhand::NodeHeader,
) -> Result<(), String> {
    let bytes = std::io::Cursor::new(data);
    if writer.mut_file(path).is_some() {
        writer.replace_file(path, bytes)
    } else {
        writer.push_file(bytes, path, header)
    }
    .map_err(|e| format!("{path}: {e}"))
}

/// A root shell on the dongle: the bind shell the hook opens on a stock one, or telnet on one that
/// runs our kernel.
pub trait Remote {
    /// An error unless the command exits 0.
    fn run(&mut self, cmd: &str) -> Result<String, String>;
    fn write_mtd(&mut self, node: &str, data: &[u8]) -> Result<(), String>;
}

/// A `/proc/mtd` line: `mtd3: 00300000 00010000 "boot"`, size and erase size in hex.
pub(crate) fn mtd_is(proc_mtd: &str, index: u8, name: &str, size: u64) -> bool {
    let prefix = format!("mtd{index}:");
    proc_mtd.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix(&prefix) else {
            return false;
        };
        let mut fields = rest.split_whitespace();
        let listed_size = fields.next().and_then(|s| u64::from_str_radix(s, 16).ok());
        let listed_name = fields.nth(1).map(|s| s.trim_matches('"'));
        listed_size == Some(size) && listed_name == Some(name)
    })
}
