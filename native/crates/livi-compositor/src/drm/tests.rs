// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::{
    Offered, assign, calibrated, choose, panel_id, panel_name, size_of, timing_mm, touch_cal_of,
    wl_output_in,
};

fn mode(w: u16, h: u16, refresh: u32, preferred: bool) -> Offered {
    Offered {
        size: (w, h),
        refresh,
        preferred,
    }
}

#[test]
fn the_setting_wins_with_its_fastest_refresh() {
    let modes = [
        mode(1920, 1080, 60, true),
        mode(1280, 720, 50, false),
        mode(1280, 720, 60, false),
    ];
    assert_eq!(choose(&modes, Some((1280, 720))), Some(2));
    assert_eq!(size_of("1024x600"), Some((1024, 600)));
    assert_eq!(size_of("auto"), None);
}

#[test]
fn without_a_setting_the_panel_stays_at_most_hd() {
    let small_panel = [mode(800, 480, 60, true), mode(640, 480, 60, false)];
    assert_eq!(choose(&small_panel, None), Some(0));
    let xga = [mode(1024, 768, 60, true), mode(800, 600, 60, false)];
    assert_eq!(
        choose(&xga, None),
        Some(0),
        "a 4:3 panel keeps its native mode below the HD pixel count"
    );
    let tv = [
        mode(3840, 2160, 60, true),
        mode(1920, 1080, 60, false),
        mode(1280, 720, 60, false),
    ];
    assert_eq!(
        choose(&tv, None),
        Some(2),
        "a 4k panel drops to 720p like the kiosk did"
    );
    assert_eq!(
        choose(&tv, Some((1366, 768))),
        Some(2),
        "a size the panel lacks falls back"
    );
    let only_big = [mode(2560, 1440, 60, true), mode(1920, 1080, 60, false)];
    assert_eq!(
        choose(&only_big, None),
        Some(1),
        "the smallest when nothing fits"
    );
    assert_eq!(choose(&[], None), None);
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn screens_take_their_connector_or_the_next_free_one() {
    let connected = names(&["HDMI-A-1", "HDMI-A-2", "DSI-1"]);
    let auto = [(0, None), (1, None)];
    assert_eq!(
        assign(&auto, &connected),
        [(0, "HDMI-A-1".into()), (1, "HDMI-A-2".into())]
    );
    let cluster_on_dsi = [(0, None), (1, Some("DSI-1".to_string()))];
    assert_eq!(
        assign(&cluster_on_dsi, &connected),
        [(0, "HDMI-A-1".into()), (1, "DSI-1".into())]
    );
    let main_on_second = [(0, Some("HDMI-A-2".to_string())), (1, None)];
    assert_eq!(
        assign(&main_on_second, &connected),
        [(0, "HDMI-A-2".into()), (1, "HDMI-A-1".into())]
    );
    let gone = [(0, None), (1, Some("HDMI-A-3".to_string()))];
    assert_eq!(
        assign(&gone, &connected),
        [(0, "HDMI-A-1".into())],
        "a chosen connector that is not there leaves the screen dark"
    );
    assert_eq!(
        assign(&auto, &names(&["HDMI-A-1"])),
        [(0, "HDMI-A-1".into())]
    );
    let pinned_cluster = [(0, None), (1, Some("HDMI-A-1".to_string()))];
    assert_eq!(
        assign(&pinned_cluster, &names(&["HDMI-A-1"])),
        [(0, "HDMI-A-1".into())],
        "main keeps the only display even when the cluster asks for it"
    );
    let main_gone = [(0, Some("DSI-1".to_string())), (1, None)];
    assert_eq!(
        assign(&main_gone, &names(&["HDMI-A-1"])),
        [(0, "HDMI-A-1".into())],
        "main never stays dark while a display is connected"
    );
    let cluster_on_first = [(0, None), (1, Some("HDMI-A-1".to_string()))];
    assert_eq!(
        assign(&cluster_on_first, &names(&["HDMI-A-1", "HDMI-A-2"])),
        [(0, "HDMI-A-2".into()), (1, "HDMI-A-1".into())],
        "main steps aside to a display nobody asked for"
    );
}

#[test]
fn a_touch_device_names_its_display_in_the_udev_data() {
    assert_eq!(
        wl_output_in("E:ID_INPUT=1\nE:WL_OUTPUT=DSI-1\nE:ID_INPUT_TOUCHSCREEN=1\n").as_deref(),
        Some("DSI-1")
    );
    assert_eq!(wl_output_in("E:ID_INPUT=1\n"), None);
    assert_eq!(wl_output_in("E:WL_OUTPUT=\n"), None);
}

#[test]
fn the_panel_name_comes_from_the_edid_descriptor() {
    let mut edid = vec![0u8; 128];
    let block = 54 + 18;
    edid[block + 3] = 0xFC;
    edid[block + 5..block + 5 + 13].copy_from_slice(b"Elecrow 8in\n ");
    assert_eq!(panel_name(&edid).as_deref(), Some("Elecrow 8in"));
    assert_eq!(panel_name(&[0u8; 128]), None);
    assert_eq!(panel_name(&[0u8; 20]), None);
}

#[test]
fn the_panel_size_falls_back_to_the_first_detailed_timing() {
    let mut edid = vec![0u8; 128];
    edid[54] = 0x04;
    edid[54 + 12] = 0x61;
    edid[54 + 13] = 0x63;
    edid[54 + 14] = 0x21;
    assert_eq!(timing_mm(&edid), Some((609, 355)));
    edid[54] = 0;
    assert_eq!(timing_mm(&edid), None);
    assert_eq!(timing_mm(&[0u8; 60]), None);
}

#[test]
fn the_panel_id_comes_from_vendor_product_and_serial() {
    let mut edid = vec![0u8; 128];
    edid[..8].copy_from_slice(&[0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0]);
    edid[8..16].copy_from_slice(&[0x10, 0xAC, 0xB1, 0xA0, 0x01, 0x00, 0x00, 0x00]);
    assert_eq!(panel_id(&edid).as_deref(), Some("DEL-a0b1-00000001"));
    assert_eq!(panel_id(&[0u8; 128]), None);
    assert_eq!(panel_id(&edid[..12]), None);
}

#[test]
fn a_touch_calibration_line_names_its_device_last() {
    let (name, cal) =
        touch_cal_of("touchcal 1 DEL-a0b1-00000001 1 0 0.02 0 0.98 0 ILITEK ILITEK-TP").unwrap();
    assert_eq!(name, "ILITEK ILITEK-TP");
    assert!(cal.enabled);
    assert_eq!(cal.display, "DEL-a0b1-00000001");
    assert_eq!(cal.matrix, [1.0, 0.0, 0.02, 0.0, 0.98, 0.0]);
    let (_, off) = touch_cal_of("touchcal 0 - 1 0 0 0 1 0 Other").unwrap();
    assert!(!off.enabled && off.display.is_empty());
    assert!(touch_cal_of("touchcal 1 - 1 0 0 0 1 Short").is_none());
    assert!(touch_cal_of("touchcal 1 - 1 0 0 0 1 0 ").is_none());
}

#[test]
fn the_calibration_works_in_screen_space() {
    let shift = [1.0, 0.0, 0.02, 0.0, 0.5, 0.25];
    assert_eq!(
        calibrated(&shift, 400.0, 240.0, 800.0, 480.0),
        (416.0, 240.0)
    );
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    assert_eq!(
        calibrated(&identity, 12.0, 34.0, 800.0, 480.0),
        (12.0, 34.0)
    );
}
