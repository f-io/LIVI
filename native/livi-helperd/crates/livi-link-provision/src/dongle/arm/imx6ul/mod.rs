//! Brings a CPC200-CCPA that already runs our root shell (busybox telnetd :2323) into the LIVI
//! Link state: keep the vendor init as `.orig`, strip the vendor userspace, install the stack
//! under /script/livi and our bring-up as the boot script, start it, verify.
//!
//! Everything runs over the dongle's NCM link. The way back is writing a backup image over the
//! rootfs, which is why `apply` reads one off the device first.

pub mod ballast;
pub mod mtd;
pub mod payload;
pub mod shell;

use std::thread::sleep;
use std::time::{Duration, Instant};

use payload::{
    BOA_CGI, BOA_LEFTOVERS, BRINGUP_MARKER, BRINGUP_REMOTE, LIVI_DIR, OBSOLETE, STOCK_BACKUP,
};
use shell::{PUSH_PORT, Shell};

/// The dongle's fallback access point, the way back in when the USB link is gone.
const AP_NAME: &str = "LIVI Link";
/// Headroom kept free on the rootfs after the stack is installed.
const SPACE_MARGIN_K: u64 = 256;
/// How long the dongle may take to come back after a reboot.
const REBOOT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Status {
    /// Already on the device with the same content.
    Current,
    Update,
    Install,
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Current => "ok",
            Status::Update => "update",
            Status::Install => "install",
        }
    }
}

pub struct FileState {
    pub remote: String,
    pub bytes: usize,
    pub status: Status,
}

pub struct Plan {
    pub identity: String,
    pub free_k: Option<u64>,
    /// Ballast to delete, with the size it frees.
    pub delete: Vec<(String, u64)>,
    /// Ballast libraries a kept binary still references.
    pub keep_libs: Vec<String>,
    pub files: Vec<FileState>,
}

impl Plan {
    /// KiB that still has to be pushed.
    pub fn push_k(&self) -> u64 {
        self.files
            .iter()
            .filter(|f| f.status != Status::Current)
            .map(|f| f.bytes as u64 / 1024)
            .sum()
    }
}

pub struct Check {
    pub what: String,
    pub ok: bool,
}

pub struct Report {
    pub checks: Vec<Check>,
    pub pair_records: String,
    pub free_k: Option<u64>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }
}

/// What `apply` would do, without changing anything.
pub fn plan(sh: &Shell) -> Result<Plan, String> {
    // Read the payload first: a missing asset is a local mistake, worth hearing about before
    // waiting on the dongle.
    let payload = payload::files();
    let identity =
        sh.sh("uname -r; cat /etc/software_version 2>/dev/null")?.replace('\n', " | ");
    let free_k = sh.df_avail_k()?;

    let mut listed: Vec<String> = ballast::FILES.iter().map(|s| s.to_string()).collect();
    listed.extend(OBSOLETE.iter().map(|s| s.to_string()));
    let present = sh.exists(&listed)?;

    let mut excluded = listed.clone();
    excluded.extend(ballast::LIBS.iter().map(|s| s.to_string()));
    let needed = ballast::needed_libs(sh, &excluded)?;
    let want: Vec<&str> = needed.iter().map(|n| ballast::stem(n)).collect();
    let (keep_libs, droppable): (Vec<String>, Vec<String>) = ballast::LIBS
        .iter()
        .map(|l| l.to_string())
        .partition(|l| want.contains(&ballast::stem(l)));
    let del_libs = sh.exists(&droppable)?;

    let doomed: Vec<String> = present.iter().chain(del_libs.iter()).cloned().collect();
    let sizes = sh.sizes(&doomed)?;
    let delete = doomed
        .iter()
        .map(|p| {
            let size = sizes.iter().find(|(path, _)| path == p).map(|(_, s)| *s).unwrap_or(0);
            (p.clone(), size)
        })
        .collect();

    let files = payload
        .into_iter()
        .map(|f| {
            let status = match sh.md5(&f.remote) {
                Some(cur) if cur == f.md5 => Status::Current,
                Some(_) => Status::Update,
                None => Status::Install,
            };
            FileState { remote: f.remote, bytes: f.data.len(), status }
        })
        .collect();

    Ok(Plan { identity, free_k, delete, keep_libs, files })
}

/// Strips, installs and starts the stack. `progress` sees one line per step.
pub fn apply(
    sh: &Shell,
    reboot: bool,
    progress: &dyn Fn(&str),
) -> Result<Report, String> {
    let plan = plan(sh)?;

    match stock_backup(sh)? {
        Backup::Saved => progress("stock init saved as start_main_service.sh.orig"),
        Backup::Present => {}
    }

    let doomed: Vec<String> = plan.delete.iter().map(|(p, _)| p.clone()).collect();
    if !doomed.is_empty() {
        sh.run(&format!("rm -f {}; sync", doomed.join(" ")), Duration::from_secs(60))?;
        // Symlinks that pointed at what just went.
        sh.run(
            "for l in /usr/lib/*.so* /lib/*.so*; do [ -L \"$l\" ] && [ ! -e \"$l\" ] && rm -f \"$l\"; done; sync",
            Duration::from_secs(60),
        )?;
        progress(&format!("stripped {} files", doomed.len()));
    }

    let need_k = plan.push_k();
    let free_k = sh.df_avail_k()?.unwrap_or(0);
    progress(&format!("rootfs free after strip: {free_k}K"));
    if free_k < need_k + SPACE_MARGIN_K {
        return Err(format!(
            "not enough flash for the stack: need {}K, have {free_k}K",
            need_k + SPACE_MARGIN_K
        ));
    }

    let current: Vec<&str> = plan
        .files
        .iter()
        .filter(|f| f.status == Status::Current)
        .map(|f| f.remote.as_str())
        .collect();
    let todo: Vec<payload::File> = payload::files()
        .into_iter()
        .filter(|f| !current.contains(&f.remote.as_str()))
        .collect();
    for (i, file) in todo.iter().enumerate() {
        sh.push(&file.data, &file.remote, PUSH_PORT + i as u16, &file.md5)?;
        progress(&format!("pushed {:>7}K  {}", file.data.len() / 1024, file.remote));
    }
    sh.sh(&format!("chmod 755 {LIVI_DIR}/*.sh {BRINGUP_REMOTE} {BOA_CGI}; sync"))?;
    // The vendor's own pages would sit next to ours and serve nothing.
    sh.sh(&format!("rm -rf {BOA_LEFTOVERS}; sync"))?;
    if set_ap_name(sh)? {
        progress(&format!("fallback AP is now \"{AP_NAME}\""));
    } else {
        progress("no Wi-Fi on this dongle, the USB link is the only way in");
    }
    if let Some(free) = sh.df_avail_k()? {
        progress(&format!("rootfs free after install: {free}K"));
    }

    // --fresh only when a binary changed: it also restarts mfid, and a host holding its MFi
    // socket would have to reconnect. Script-only changes keep seedrng/mfid running.
    let fresh = if todo.iter().any(|f| f.remote.ends_with(".gz")) { " --fresh" } else { "" };
    start_stack(sh, fresh)?;
    progress(&format!("stack log:\n{}", sh.sh("cat /tmp/livi-link.log 2>/dev/null")?));

    if reboot {
        progress("rebooting");
        sh.sh("sync; (sleep 1; reboot) >/dev/null 2>&1 &")?;
        sleep(Duration::from_secs(10));
        wait_for_dongle(sh, progress)?;
        sleep(Duration::from_secs(5));
    }
    verify(sh)
}

/// Checks the installed files and the running stack.
pub fn verify(sh: &Shell) -> Result<Report, String> {
    let mut checks = Vec::new();
    for file in payload::files() {
        let ok = sh.md5(&file.remote).as_deref() == Some(file.md5.as_str());
        checks.push(Check { what: file.remote, ok });
    }
    let ps = sh.sh(&format!(
        "ps | grep -E '{}' | grep -v grep",
        payload::STACK_PROCESSES.join("|")
    ))?;
    for name in payload::STACK_PROCESSES {
        checks.push(Check { what: format!("process {name}"), ok: ps.contains(name) });
    }
    for port in payload::STACK_PORTS {
        checks.push(Check { what: format!("port {port}"), ok: sh.port_open(port) });
    }
    let pair_records = sh.sh("ls /var/lib/lockdown 2>/dev/null")?.replace('\n', " ");
    Ok(Report { checks, pair_records, free_k: sh.df_avail_k()? })
}

enum Backup {
    Saved,
    Present,
}

/// Keeps the stock init as `.orig`, taken from the device itself — on a stock dongle the file
/// still is the vendor's. Refuses when our own boot script is already installed without one,
/// because then the stock init only exists in the mtd backup.
fn stock_backup(sh: &Shell) -> Result<Backup, String> {
    let out = sh.sh(&format!(
        "if [ -e {STOCK_BACKUP} ]; then echo have; \
         elif grep -q '{BRINGUP_MARKER}' {BRINGUP_REMOTE} 2>/dev/null; then echo ours; \
         else cp {BRINGUP_REMOTE} {STOCK_BACKUP} && sync && echo saved; fi"
    ))?;
    match out.trim() {
        "have" => Ok(Backup::Present),
        "saved" => Ok(Backup::Saved),
        "ours" => Err(format!(
            "{BRINGUP_REMOTE} is already our boot script and {STOCK_BACKUP} is missing — \
             restore the stock rootfs from the mtd backup first"
        )),
        other => Err(format!("could not save the stock init: {other}")),
    }
}

/// Names the fallback AP, which is the one a dongle boots with. The host renames it to the car's
/// name once it takes the AP over, so this name showing up means nothing is driving the dongle.
fn set_ap_name(sh: &Shell) -> Result<bool, String> {
    // Some dongles have no Wi-Fi at all, and then there is no config to name anything in.
    if sh.sh("[ -f /etc/hostapd.conf ] && echo yes || echo no")?.trim() != "yes" {
        return Ok(false);
    }
    // Rebuilt rather than edited in place, so nothing has to survive sed quoting.
    sh.sh(&format!(
        "{{ grep -v '^ssid=' /etc/hostapd.conf; echo 'ssid={AP_NAME}'; }} > /tmp/hostapd.conf \
         && cp /tmp/hostapd.conf /etc/hostapd.conf && rm -f /tmp/hostapd.conf && sync"
    ))?;
    Ok(true)
}

/// Runs the stack from its persistent home, killing any hand-started copies first. Detached, so
/// the daemons do not belong to this shell session and survive it closing.
fn start_stack(sh: &Shell, fresh: &str) -> Result<(), String> {
    sh.run(
        &format!(
            "pkill -f /usr/sbin/mfid; pkill -f livi_link_up; pkill -f /tmp/livi-usbproxy; \
             pkill -f /tmp/l2fwd; pkill -f /tmp/seedrng; pkill -f /tmp/mdnsd; pkill -f l2fwd-watch; \
             sleep 1; setsid sh {LIVI_DIR}/livi-link.sh{fresh} </dev/null >/tmp/livi-link.log 2>&1 & \
             echo started"
        ),
        Duration::from_secs(90),
    )?;
    // Unpacking ~3.6 MB on this CPU takes a few seconds.
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        sleep(Duration::from_secs(2));
        if payload::STACK_PORTS.iter().all(|p| sh.port_open(*p)) {
            break;
        }
    }
    Ok(())
}

fn wait_for_dongle(sh: &Shell, progress: &dyn Fn(&str)) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < REBOOT_TIMEOUT {
        if sh.port_open(shell::TELNET_PORT) {
            progress(&format!("dongle back after {}s", start.elapsed().as_secs()));
            return Ok(());
        }
        sleep(Duration::from_secs(3));
    }
    Err(format!("dongle did not come back within {}s", REBOOT_TIMEOUT.as_secs()))
}
