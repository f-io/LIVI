// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use core::ffi::c_void;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

use super::{Fault, OutputEvent};

type AudioObjectId = u32;
type OsStatus = i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

type Listener = extern "C" fn(AudioObjectId, u32, *const Address, *mut c_void) -> OsStatus;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectHasProperty(id: AudioObjectId, address: *const Address) -> u8;
    fn AudioObjectGetPropertyDataSize(
        id: AudioObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> OsStatus;
    fn AudioObjectGetPropertyData(
        id: AudioObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> OsStatus;
    fn AudioObjectSetPropertyData(
        id: AudioObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> OsStatus;
    fn AudioObjectAddPropertyListener(
        id: AudioObjectId,
        address: *const Address,
        listener: Listener,
        client: *mut c_void,
    ) -> OsStatus;
    fn AudioObjectRemovePropertyListener(
        id: AudioObjectId,
        address: *const Address,
        listener: Listener,
        client: *mut c_void,
    ) -> OsStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SYSTEM: AudioObjectId = 1;
const UNKNOWN: AudioObjectId = 0;
const GLOBAL: u32 = fourcc(b"glob");
const OUTPUT: u32 = fourcc(b"outp");
const MAIN: u32 = 0;
const UTF8: u32 = 0x0800_0100;

const DEFAULT_OUTPUT: Address = Address {
    selector: fourcc(b"dOut"),
    scope: GLOBAL,
    element: MAIN,
};
const DEVICES: Address = Address {
    selector: fourcc(b"dev#"),
    scope: GLOBAL,
    element: MAIN,
};
const UID_TO_DEVICE: Address = Address {
    selector: fourcc(b"uidd"),
    scope: GLOBAL,
    element: MAIN,
};
const VOLUME: Address = Address {
    selector: fourcc(b"vmvc"),
    scope: OUTPUT,
    element: MAIN,
};

fn checked(status: OsStatus, what: &str) -> Result<(), Fault> {
    if status == 0 {
        Ok(())
    } else {
        Err(Fault::Other(format!("{what}: OSStatus {status}")))
    }
}

fn device_id(device: &str) -> Result<AudioObjectId, Fault> {
    let mut id = UNKNOWN;
    let mut size = size_of::<AudioObjectId>() as u32;
    let out = (&raw mut id).cast();
    if device.is_empty() {
        // SAFETY: the address and the out buffer are valid for the call.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM,
                &DEFAULT_OUTPUT,
                0,
                core::ptr::null(),
                &mut size,
                out,
            )
        };
        checked(status, "default output")?;
        return if id == UNKNOWN {
            Err(Fault::Other("no default output".into()))
        } else {
            Ok(id)
        };
    }
    // SAFETY: the bytes outlive the call, CoreFoundation copies them.
    let uid = unsafe {
        CFStringCreateWithBytes(
            core::ptr::null(),
            device.as_ptr(),
            device.len() as isize,
            UTF8,
            0,
        )
    };
    if uid.is_null() {
        return Err(Fault::Other(format!("{device} is no valid UID")));
    }
    // SAFETY: the qualifier is a CFStringRef, as the property expects.
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM,
            &UID_TO_DEVICE,
            size_of::<*const c_void>() as u32,
            (&raw const uid).cast(),
            &mut size,
            out,
        )
    };
    // SAFETY: created above, released once.
    unsafe { CFRelease(uid) };
    checked(status, "device by UID")?;
    if id == UNKNOWN {
        Err(Fault::Missing)
    } else {
        Ok(id)
    }
}

fn has_volume(id: AudioObjectId) -> bool {
    // SAFETY: a plain query on a valid address.
    unsafe { AudioObjectHasProperty(id, &VOLUME) != 0 }
}

fn read(device: &str) -> Result<f64, Fault> {
    let id = device_id(device)?;
    if !has_volume(id) {
        return Err(Fault::Other("the output has no volume control".into()));
    }
    let mut level = 0f32;
    let mut size = size_of::<f32>() as u32;
    // SAFETY: the out buffer holds the Float32 the property is.
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            &VOLUME,
            0,
            core::ptr::null(),
            &mut size,
            (&raw mut level).cast(),
        )
    };
    checked(status, "volume")?;
    Ok(whole_percent(f64::from(level)))
}

fn whole_percent(level: f64) -> f64 {
    (level.clamp(0.0, 1.0) * 100.0).round() / 100.0
}

fn write(device: &str, level: f64) -> Result<(), Fault> {
    let id = device_id(device)?;
    if !has_volume(id) {
        return Err(Fault::Other("the output has no volume control".into()));
    }
    let level = level.clamp(0.0, 1.0) as f32;
    // SAFETY: the data is the Float32 the property is.
    let status = unsafe {
        AudioObjectSetPropertyData(
            id,
            &VOLUME,
            0,
            core::ptr::null(),
            size_of::<f32>() as u32,
            (&raw const level).cast(),
        )
    };
    checked(status, "volume")
}

pub async fn get(device: &str) -> Result<f64, Fault> {
    let device = device.to_string();
    tokio::task::spawn_blocking(move || read(&device))
        .await
        .map_err(|e| Fault::Other(e.to_string()))?
}

pub async fn set(device: &str, level: f64) -> Result<(), Fault> {
    let device = device.to_string();
    tokio::task::spawn_blocking(move || write(&device, level))
        .await
        .map_err(|e| Fault::Other(e.to_string()))?
}

fn output_devices() -> Vec<AudioObjectId> {
    let mut size = 0u32;
    // SAFETY: a size query on a valid address.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(SYSTEM, &DEVICES, 0, core::ptr::null(), &mut size)
    };
    if status != 0 {
        return Vec::new();
    }
    let mut ids = vec![UNKNOWN; size as usize / size_of::<AudioObjectId>()];
    // SAFETY: the buffer is as large as CoreAudio said.
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM,
            &DEVICES,
            0,
            core::ptr::null(),
            &mut size,
            ids.as_mut_ptr().cast(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ids.truncate(size as usize / size_of::<AudioObjectId>());
    ids.into_iter().filter(|&id| has_volume(id)).collect()
}

struct Watcher {
    tx: mpsc::UnboundedSender<OutputEvent>,
    listened: Mutex<HashSet<AudioObjectId>>,
    stopped: AtomicBool,
}

impl Watcher {
    fn client(&'static self) -> *mut c_void {
        (self as *const Self).cast_mut().cast()
    }

    fn listen_to_outputs(&'static self) {
        let present = output_devices();
        let mut listened = self.listened.lock().unwrap_or_else(|e| e.into_inner());
        listened.retain(|id| present.contains(id));
        for id in present {
            // SAFETY: the client is 'static, the callback reads it as a Watcher.
            if !listened.contains(&id)
                && unsafe { AudioObjectAddPropertyListener(id, &VOLUME, on_change, self.client()) }
                    == 0
            {
                listened.insert(id);
            }
        }
    }
}

extern "C" fn on_change(
    _id: AudioObjectId,
    count: u32,
    addresses: *const Address,
    client: *mut c_void,
) -> OsStatus {
    // SAFETY: the client is the leaked Watcher the listener was added with.
    let watcher: &'static Watcher = unsafe { &*client.cast::<Watcher>() };
    if watcher.stopped.load(Ordering::Relaxed) || addresses.is_null() {
        return 0;
    }
    // SAFETY: CoreAudio hands `count` addresses.
    let addresses = unsafe { core::slice::from_raw_parts(addresses, count as usize) };
    for address in addresses {
        let event = if address.selector == DEVICES.selector {
            watcher.listen_to_outputs();
            OutputEvent::New
        } else {
            OutputEvent::Changed
        };
        let _ = watcher.tx.send(event);
    }
    0
}

pub struct Watch(&'static Watcher);

impl Drop for Watch {
    fn drop(&mut self) {
        let watcher = self.0;
        watcher.stopped.store(true, Ordering::Relaxed);
        let client = watcher.client();
        let listened: Vec<AudioObjectId> = watcher
            .listened
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .collect();
        // SAFETY: removes exactly what `watch` and `listen_to_outputs` added.
        unsafe {
            AudioObjectRemovePropertyListener(SYSTEM, &DEVICES, on_change, client);
            AudioObjectRemovePropertyListener(SYSTEM, &DEFAULT_OUTPUT, on_change, client);
            for id in listened {
                AudioObjectRemovePropertyListener(id, &VOLUME, on_change, client);
            }
        }
    }
}

pub fn watch(tx: mpsc::UnboundedSender<OutputEvent>) -> Watch {
    let watcher: &'static Watcher = Box::leak(Box::new(Watcher {
        tx,
        listened: Mutex::new(HashSet::new()),
        stopped: AtomicBool::new(false),
    }));
    let client = watcher.client();
    // SAFETY: the client is 'static, the callback reads it as a Watcher.
    unsafe {
        AudioObjectAddPropertyListener(SYSTEM, &DEVICES, on_change, client);
        AudioObjectAddPropertyListener(SYSTEM, &DEFAULT_OUTPUT, on_change, client);
    }
    watcher.listen_to_outputs();
    Watch(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_that_is_not_there_is_missing() {
        assert!(matches!(
            device_id("LIVI-no-such-device"),
            Err(Fault::Missing)
        ));
    }

    #[test]
    fn the_level_is_whole_percent() {
        assert_eq!(whole_percent(0.660_000_026), 0.66);
        assert_eq!(whole_percent(1.2), 1.0);
    }
}
