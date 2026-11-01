// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn car_name(hostname: &str, max: usize) -> Option<String> {
    let name = hostname.split('.').next().unwrap_or("").trim();
    if name.is_empty() || name.eq_ignore_ascii_case("localhost") {
        return None;
    }
    Some(name.chars().take(max).collect())
}

pub fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length and gethostname writes no more.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..end].to_vec()).ok()
}

#[cfg(any(target_os = "linux", test))]
fn edid_native_px(edid: &[u8]) -> Option<(u32, u32)> {
    if edid.len() < 128 {
        return None;
    }
    let dtd = &edid[0x36..0x48];
    if dtd[0] == 0 && dtd[1] == 0 {
        return None;
    }
    let b = |i: usize| u32::from(dtd[i]);
    let width_mm = ((b(14) & 0xf0) << 4) | b(12);
    let height_mm = ((b(14) & 0x0f) << 8) | b(13);
    let width = ((b(4) & 0xf0) << 4) | b(2);
    let height = ((b(7) & 0xf0) << 4) | b(5);
    (width_mm > 0 && height_mm > 0 && width > 0 && height > 0).then_some((width, height))
}

#[cfg(any(target_os = "linux", test))]
fn forced_edid_connector(cmdline: &str) -> Option<&str> {
    let value = cmdline
        .split_whitespace()
        .find_map(|t| t.strip_prefix("drm.edid_firmware="))?;
    let (connector, _) = value.split_once(':')?;
    let valid = !connector.is_empty()
        && connector
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then_some(connector)
}

#[cfg(target_os = "linux")]
pub fn panel_native_px() -> Option<(u32, u32)> {
    use std::fs;

    let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let connected: Vec<String> = fs::read_dir("/sys/class/drm")
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| is_connector(n))
        .filter(|n| {
            fs::read_to_string(format!("/sys/class/drm/{n}/status"))
                .is_ok_and(|s| s.trim() == "connected")
        })
        .collect();
    let pick = match forced_edid_connector(&cmdline) {
        Some(forced) => connected
            .iter()
            .find(|n| n.ends_with(&format!("-{forced}")))?,
        None if connected.len() == 1 => &connected[0],
        None => return None,
    };
    let px = edid_native_px(&fs::read(format!("/sys/class/drm/{pick}/edid")).ok()?)?;
    println!("[core] EDID of {pick}: {}x{} px", px.0, px.1);
    Some(px)
}

#[cfg(not(target_os = "linux"))]
pub fn panel_native_px() -> Option<(u32, u32)> {
    None
}

#[cfg(target_os = "macos")]
pub fn screen_panel() -> Option<livi_media::compositor::Panel> {
    use std::ffi::c_void;

    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayScreenSize(display: u32) -> CGSize;
        fn CGDisplayCopyDisplayMode(display: u32) -> *mut c_void;
        fn CGDisplayModeGetPixelWidth(mode: *mut c_void) -> usize;
        fn CGDisplayModeGetPixelHeight(mode: *mut c_void) -> usize;
        fn CGDisplayModeRelease(mode: *mut c_void);
    }

    // SAFETY: plain CoreGraphics queries on the main display, the copied mode is released once.
    let (mm, px) = unsafe {
        let display = CGMainDisplayID();
        let mm = CGDisplayScreenSize(display);
        let mode = CGDisplayCopyDisplayMode(display);
        if mode.is_null() {
            return None;
        }
        let px = (
            CGDisplayModeGetPixelWidth(mode),
            CGDisplayModeGetPixelHeight(mode),
        );
        CGDisplayModeRelease(mode);
        (mm, px)
    };
    let panel = screen_panel_from(mm.width, mm.height, px.0, px.1)?;
    println!(
        "[core] screen: {}x{} mm over {}x{} px",
        panel.width_mm, panel.height_mm, panel.width_px, panel.height_px
    );
    Some(panel)
}

#[cfg(not(target_os = "macos"))]
pub fn screen_panel() -> Option<livi_media::compositor::Panel> {
    None
}

#[cfg(any(target_os = "macos", test))]
fn screen_panel_from(
    width_mm: f64,
    height_mm: f64,
    width_px: usize,
    height_px: usize,
) -> Option<livi_media::compositor::Panel> {
    let panel = livi_media::compositor::Panel {
        width_mm: width_mm.round() as u32,
        height_mm: height_mm.round() as u32,
        width_px: u32::try_from(width_px).ok()?,
        height_px: u32::try_from(height_px).ok()?,
    };
    let known = panel.width_mm > 0 && panel.height_mm > 0 && panel.width_px > 0;
    (known && panel.height_px > 0).then_some(panel)
}

#[cfg(any(target_os = "linux", test))]
fn is_connector(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("card") else {
        return false;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && rest[digits..].starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_needs_both_sizes() {
        let panel = screen_panel_from(286.4, 178.9, 2560, 1600).unwrap();
        assert_eq!((panel.width_mm, panel.height_mm), (286, 179));
        assert_eq!((panel.width_px, panel.height_px), (2560, 1600));
        assert_eq!(screen_panel_from(0.0, 178.9, 2560, 1600), None);
        assert_eq!(screen_panel_from(286.4, 178.9, 2560, 0), None);
    }

    #[test]
    fn the_screen_is_read_or_left_alone() {
        if let Some(panel) = screen_panel() {
            assert!(panel.width_mm > 0 && panel.width_px > 0);
        }
    }

    #[test]
    fn car_name_from_the_host() {
        assert_eq!(car_name("golf.fritz.box", 20).as_deref(), Some("golf"));
        assert_eq!(car_name("  ", 20), None);
        assert_eq!(car_name("LocalHost.lan", 20), None);
        assert_eq!(
            car_name("a-very-long-host-name-indeed", 20).as_deref(),
            Some("a-very-long-host-nam")
        );
    }

    fn edid(width: u32, height: u32, width_mm: u32, height_mm: u32) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        let d = 0x36;
        e[d] = 0x01;
        e[d + 2] = (width & 0xff) as u8;
        e[d + 4] = ((width >> 4) & 0xf0) as u8;
        e[d + 5] = (height & 0xff) as u8;
        e[d + 7] = ((height >> 4) & 0xf0) as u8;
        e[d + 12] = (width_mm & 0xff) as u8;
        e[d + 13] = (height_mm & 0xff) as u8;
        e[d + 14] = (((width_mm >> 4) & 0xf0) | ((height_mm >> 8) & 0x0f)) as u8;
        e
    }

    #[test]
    fn edid_gives_the_native_mode() {
        assert_eq!(
            edid_native_px(&edid(1920, 1080, 344, 194)),
            Some((1920, 1080))
        );
        assert_eq!(edid_native_px(&edid(1024, 600, 0, 0)), None);
        assert_eq!(edid_native_px(&edid(1920, 1080, 344, 194)[..127]), None);
        let mut no_timing = edid(1920, 1080, 344, 194);
        no_timing[0x36] = 0;
        assert_eq!(edid_native_px(&no_timing), None);
    }

    #[test]
    fn forced_connector_from_the_command_line() {
        let line = "console=tty1 drm.edid_firmware=HDMI-A-1:edid/panel.bin quiet";
        assert_eq!(forced_edid_connector(line), Some("HDMI-A-1"));
        assert_eq!(
            forced_edid_connector("drm.edid_firmware=edid/panel.bin"),
            None
        );
        assert_eq!(forced_edid_connector("quiet"), None);
    }

    #[test]
    fn connectors_are_card_number_dash() {
        assert!(is_connector("card0-HDMI-A-1"));
        assert!(is_connector("card12-DSI-1"));
        assert!(!is_connector("card0"));
        assert!(!is_connector("renderD128"));
        assert!(!is_connector("card-x"));
    }
}
