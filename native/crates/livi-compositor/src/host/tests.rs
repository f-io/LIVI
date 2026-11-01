// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::{CursorIcon, HostState, cursor_requested, fit_aspect, share_of_panel, shown_cursor};

fn over_ui() -> HostState {
    let mut host = HostState::new();
    host.pointer_on_inner = true;
    host
}

#[test]
fn the_default_arrow_before_the_page_hides_it_stays_hidden() {
    let mut host = over_ui();
    cursor_requested(&mut host, Some(CursorIcon::Default));
    assert_eq!(shown_cursor(&host), None);
}

#[test]
fn the_ui_shows_the_pointer_once_it_has_hidden_it() {
    let mut host = over_ui();
    cursor_requested(&mut host, None);
    cursor_requested(&mut host, Some(CursorIcon::Pointer));
    assert_eq!(shown_cursor(&host), Some(CursorIcon::Pointer));
}

#[test]
fn leaving_the_ui_keeps_the_hidden_pointer() {
    let mut host = over_ui();
    cursor_requested(&mut host, None);
    host.pointer_on_inner = false;
    cursor_requested(&mut host, Some(CursorIcon::Default));
    assert_eq!(shown_cursor(&host), None);
}

#[test]
fn the_decoration_always_shows_the_arrow() {
    let mut host = over_ui();
    cursor_requested(&mut host, None);
    host.pointer_on_inner = false;
    host.pointer_on_deco = true;
    assert_eq!(shown_cursor(&host), Some(CursorIcon::Default));
}

#[test]
fn a_floating_window_keeps_the_stream_shape_below_the_titlebar() {
    assert_eq!(fit_aspect((1280, 752), 32, (1280, 720)), (1280, 752));
    assert_eq!(fit_aspect((2000, 752), 32, (1280, 720)), (1280, 752));
    assert_eq!(fit_aspect((1280, 1000), 32, (1280, 720)), (1280, 752));
    assert_eq!(fit_aspect((1000, 512), 32, (800, 480)), (800, 512));
    assert_eq!(fit_aspect((1920, 1200), 0, (1920, 1080)), (1920, 1080));
}

#[test]
fn the_minimum_size_wins_over_the_offer() {
    assert_eq!(fit_aspect((100, 50), 32, (16, 9)), (320, 212));
    assert_eq!(fit_aspect((100, 50), 32, (1, 4)), (320, 1312));
}

#[test]
fn a_window_gets_the_millimetres_of_the_part_it_covers() {
    assert_eq!(
        share_of_panel((600, 340), (1280, 752), (2560, 1440)),
        (300, 178)
    );
}

#[test]
fn a_fullscreen_window_keeps_the_whole_panel() {
    assert_eq!(
        share_of_panel((600, 340), (2560, 1440), (2560, 1440)),
        (600, 340)
    );
}
