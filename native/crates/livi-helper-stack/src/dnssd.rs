// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ffi::{CStr, CString, c_char, c_void};
use std::io;
use std::time::{Duration, Instant};

type ServiceRef = *mut c_void;

const NO_ERROR: i32 = 0;
const FLAG_ADD: u32 = 0x2;

type BrowseReply = extern "C" fn(
    ServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);
type ResolveReply = extern "C" fn(
    ServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    u16,
    u16,
    *const u8,
    *mut c_void,
);

unsafe extern "C" {
    fn DNSServiceRegister(
        sd_ref: *mut ServiceRef,
        flags: u32,
        interface_index: u32,
        name: *const c_char,
        regtype: *const c_char,
        domain: *const c_char,
        host: *const c_char,
        port: u16,
        txt_len: u16,
        txt_record: *const c_void,
        callback: *const c_void,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceBrowse(
        sd_ref: *mut ServiceRef,
        flags: u32,
        interface_index: u32,
        regtype: *const c_char,
        domain: *const c_char,
        callback: BrowseReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceResolve(
        sd_ref: *mut ServiceRef,
        flags: u32,
        interface_index: u32,
        name: *const c_char,
        regtype: *const c_char,
        domain: *const c_char,
        callback: ResolveReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(sd_ref: ServiceRef) -> i32;
    fn DNSServiceProcessResult(sd_ref: ServiceRef) -> i32;
    fn DNSServiceRefDeallocate(sd_ref: ServiceRef);
}

struct Service(ServiceRef);

// SAFETY: a DNSServiceRef may move between threads as long as one thread uses it at a time.
unsafe impl Send for Service {}

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: the ref came from a successful DNSService call and is freed once.
        unsafe { DNSServiceRefDeallocate(self.0) };
    }
}

impl Service {
    fn next(&self, until: Instant) -> bool {
        let Some(left) = until.checked_duration_since(Instant::now()) else {
            return false;
        };
        let mut pfd = libc::pollfd {
            // SAFETY: the ref is live.
            fd: unsafe { DNSServiceRefSockFD(self.0) },
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: pfd is one valid pollfd.
        if unsafe { libc::poll(&mut pfd, 1, ms) } <= 0 {
            return false;
        }
        // SAFETY: the ref is live and its socket is readable.
        unsafe { DNSServiceProcessResult(self.0) == NO_ERROR }
    }
}

fn cstr(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|_| io::Error::other(format!("{s:?} holds a NUL")))
}

fn failed(what: &str, code: i32) -> io::Error {
    io::Error::other(format!("{what} failed: dns_sd error {code}"))
}

fn txt_record(entries: &[String]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    for entry in entries {
        let len = u8::try_from(entry.len())
            .map_err(|_| io::Error::other(format!("TXT entry {entry:?} is too long")))?;
        out.push(len);
        out.extend_from_slice(entry.as_bytes());
    }
    Ok(out)
}

pub struct Registration(#[allow(dead_code)] Service);

pub fn register(name: &str, regtype: &str, port: u16, txt: &[String]) -> io::Result<Registration> {
    let (name, regtype) = (cstr(name)?, cstr(regtype)?);
    let txt = txt_record(txt)?;
    let txt_len = u16::try_from(txt.len()).map_err(|_| io::Error::other("TXT record too long"))?;
    let mut sd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the call, mDNSResponder copies what it keeps.
    let code = unsafe {
        DNSServiceRegister(
            &mut sd,
            0,
            0,
            name.as_ptr(),
            regtype.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            port.to_be(),
            txt_len,
            txt.as_ptr().cast(),
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if code != NO_ERROR {
        return Err(failed("DNSServiceRegister", code));
    }
    Ok(Registration(Service(sd)))
}

extern "C" fn browsed(
    _: ServiceRef,
    flags: u32,
    iface: u32,
    error: i32,
    name: *const c_char,
    _: *const c_char,
    _: *const c_char,
    context: *mut c_void,
) {
    if error != NO_ERROR || flags & FLAG_ADD == 0 || name.is_null() {
        return;
    }
    // SAFETY: context is the Vec browse_on passed in, alive until the ref is freed.
    let found = unsafe { &mut *context.cast::<Vec<(String, u32)>>() };
    // SAFETY: mDNSResponder hands a NUL-terminated name.
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    if !found.contains(&(name.clone(), iface)) {
        found.push((name, iface));
    }
}

pub fn browse(regtype: &str, wait: Duration) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (name, _) in browse_on(regtype, wait) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

pub fn browse_on(regtype: &str, wait: Duration) -> Vec<(String, u32)> {
    let mut found = Vec::new();
    let Ok(regtype) = cstr(regtype) else {
        return found;
    };
    let mut sd = std::ptr::null_mut();
    // SAFETY: found outlives the ref, which the Service below frees before returning.
    let code = unsafe {
        DNSServiceBrowse(
            &mut sd,
            0,
            0,
            regtype.as_ptr(),
            std::ptr::null(),
            browsed,
            (&raw mut found).cast(),
        )
    };
    if code == NO_ERROR {
        let (service, until) = (Service(sd), Instant::now() + wait);
        while service.next(until) {}
    }
    found
}

pub struct Resolved {
    pub host: String,
    pub port: u16,
    pub txt: Vec<String>,
}

extern "C" fn resolved(
    _: ServiceRef,
    _: u32,
    _: u32,
    error: i32,
    _: *const c_char,
    host: *const c_char,
    port: u16,
    txt_len: u16,
    txt: *const u8,
    context: *mut c_void,
) {
    if error != NO_ERROR || host.is_null() {
        return;
    }
    // SAFETY: context is the Option resolve passed in, alive until the ref is freed.
    let slot = unsafe { &mut *context.cast::<Option<Resolved>>() };
    // SAFETY: mDNSResponder hands a NUL-terminated host and txt_len bytes of TXT record.
    let (host, txt) = unsafe {
        (
            CStr::from_ptr(host).to_string_lossy().into_owned(),
            std::slice::from_raw_parts(txt, usize::from(txt_len)),
        )
    };
    *slot = Some(Resolved {
        host: host.trim_end_matches('.').to_string(),
        port: u16::from_be(port),
        txt: txt_entries(txt),
    });
}

fn txt_entries(mut record: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    while let Some((&len, rest)) = record.split_first() {
        let len = usize::from(len).min(rest.len());
        out.push(String::from_utf8_lossy(&rest[..len]).into_owned());
        record = &rest[len..];
    }
    out
}

pub fn resolve(name: &str, regtype: &str, iface: u32, wait: Duration) -> Option<Resolved> {
    let (name, regtype, domain) = (cstr(name).ok()?, cstr(regtype).ok()?, cstr("local.").ok()?);
    let mut slot: Option<Resolved> = None;
    let mut sd = std::ptr::null_mut();
    // SAFETY: slot outlives the ref, which the Service below frees before returning.
    let code = unsafe {
        DNSServiceResolve(
            &mut sd,
            0,
            iface,
            name.as_ptr(),
            regtype.as_ptr(),
            domain.as_ptr(),
            resolved,
            (&raw mut slot).cast(),
        )
    };
    if code != NO_ERROR {
        return None;
    }
    let (service, until) = (Service(sd), Instant::now() + wait);
    while slot.is_none() && service.next(until) {}
    slot
}

pub fn ifname(index: u32) -> Option<String> {
    let mut buf = [0 as c_char; libc::IF_NAMESIZE];
    // SAFETY: buf holds IF_NAMESIZE bytes, as if_indextoname requires.
    let name = unsafe { libc::if_indextoname(index, buf.as_mut_ptr()) };
    (!name.is_null()).then(|| {
        // SAFETY: if_indextoname returned buf holding a NUL-terminated name.
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_entries_round_trip() {
        let entries = vec!["id=aa:bb".to_string(), "flags=0x4".to_string()];
        assert_eq!(txt_entries(&txt_record(&entries).unwrap()), entries);
    }
}
