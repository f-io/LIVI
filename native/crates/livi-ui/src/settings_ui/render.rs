// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

pub(super) fn color(c: [u8; 3]) -> Color {
    Color::from_rgb_u8(c[0], c[1], c[2])
}

pub(super) fn row_kind(k: ModelRowKind) -> RowKind {
    match k {
        ModelRowKind::Route => RowKind::Route,
        ModelRowKind::Value => RowKind::Value,
        ModelRowKind::Switch => RowKind::Toggle,
        ModelRowKind::Slider => RowKind::Slider,
        ModelRowKind::KeyBinding => RowKind::KeyBinding,
        ModelRowKind::Custom => RowKind::Custom,
        ModelRowKind::Devices => RowKind::Devices,
        ModelRowKind::Info => RowKind::Info,
        ModelRowKind::Button => RowKind::Button,
        ModelRowKind::Progress => RowKind::Progress,
        ModelRowKind::Item => RowKind::Item,
        ModelRowKind::Device => RowKind::Device,
        ModelRowKind::Picture => RowKind::Picture,
        ModelRowKind::Pick => RowKind::Pick,
    }
}

pub(super) fn logo(b64: &str) -> Option<Image> {
    LOGOS.with_borrow_mut(|cache| {
        cache
            .entry(b64.to_string())
            .or_insert_with(|| crate::media::decode(b64).map(Image::from_rgba8))
            .clone()
    })
}

pub fn focus_stops(r: &Row) -> usize {
    match r.kind {
        ModelRowKind::Item => r.tools.len(),
        ModelRowKind::Device => usize::from(r.enabled) + r.tools.len(),
        _ if r.focusable() => 1,
        _ => 0,
    }
}

pub(super) fn row_data(r: &Row, focus_at: i32) -> RowData {
    let dev = r.device.as_ref();
    let picture = r.kind == ModelRowKind::Picture;
    let image = if picture {
        logo(&r.value)
    } else {
        dev.map_or(r.icon, |d| Some(d.protocol_icon)).and_then(icon)
    };
    let transport = dev.and_then(|d| d.transport_icon).and_then(icon);
    let swatch = r.swatch.as_deref().and_then(rgb);
    let badge_logo = r
        .badge
        .as_ref()
        .and_then(|b| b.logo.as_deref())
        .and_then(logo);
    let tools: Vec<ToolData> = r
        .tools
        .iter()
        .map(|t| ToolData {
            icon: icon(t.icon).unwrap_or_default(),
            on: t.on,
            has_tint: t.tint.is_some(),
            tint: t.tint.map(color).unwrap_or_default(),
            enabled: t.enabled,
            switch: t.switch,
        })
        .collect();
    RowData {
        node: r.node as i32,
        focus_at,
        button: r.button.clone().into(),
        outlined: r.outlined,
        dimmed: r.dimmed,
        has_badge: r.badge.is_some(),
        badge_initials: r
            .badge
            .as_ref()
            .map(|b| b.initials.clone())
            .unwrap_or_default()
            .into(),
        badge_color: r.badge.as_ref().map(|b| color(b.color)).unwrap_or_default(),
        badge_has_logo: badge_logo.is_some(),
        badge_logo: badge_logo.unwrap_or_default(),
        tools: ModelRc::from(Rc::new(VecModel::from(tools))),
        mono: r.mono,
        alert: r.alert,
        tool_offset: match r.kind {
            ModelRowKind::Item => 0,
            ModelRowKind::Device => i32::from(r.enabled),
            _ => -1,
        },
        icon_active: dev.is_some_and(|d| d.presence == Presence::Active),
        has_transport: transport.is_some(),
        transport: transport.unwrap_or_default(),
        has_battery: dev.is_some_and(|d| d.battery.is_some()),
        battery: dev.and_then(|d| d.battery).map_or(0, |b| i32::from(b.0)),
        charging: dev.and_then(|d| d.battery).is_some_and(|b| b.1),
        has_signal: dev.is_some_and(|d| d.signal.is_some()),
        signal: dev.and_then(|d| d.signal).map_or(0, i32::from),
        presence: match dev.map(|d| d.presence) {
            Some(Presence::Active) => 0,
            Some(Presence::Available) => 1,
            _ => 2,
        },
        kind: row_kind(r.kind),
        label: r.label.clone().into(),
        has_icon: image.is_some(),
        icon: image.unwrap_or_default(),
        value: if picture {
            SharedString::new()
        } else {
            r.value.clone().into()
        },
        checked: r.checked,
        level: r.level,
        enabled: r.enabled,
        has_swatch: swatch.is_some(),
        swatch: swatch.map(color).unwrap_or_default(),
    }
}

pub(super) fn custom_name(page: CustomPage) -> &'static str {
    match page {
        CustomPage::About => "About",
        CustomPage::CarReadings => "Car readings",
        CustomPage::ColorCalibration => "Color calibration",
        CustomPage::ContrastGammaCalibration => "Contrast / Gamma",
        CustomPage::DabReceiver => "DAB receiver",
        CustomPage::DabStations => "DAB stations",
        CustomPage::DashboardLayouts => "Dashboard layouts",
        CustomPage::GpsHwInfo => "GPS hardware",
        CustomPage::GpsInfo => "GPS info",
        CustomPage::IconUploader => "UI icon",
        CustomPage::MfiCertificate => "MFi certificate",
        CustomPage::PowerOff => "Power off",
        CustomPage::Restart => "Restart",
        CustomPage::SoftwareUpdate => "Software update",
        CustomPage::Touch(_) => "Touch",
        CustomPage::WifiLinkInfo => "Wi-Fi link",
    }
}

pub(super) fn show_color(s: &SettingsState, hsl: Hsl, c: [u8; 3], custom: bool) {
    s.set_color_preview(color(c));
    s.set_color_hex(hex(c).into());
    s.set_color_custom(custom);
    s.set_color_hue(hsl.h as f32);
    s.set_color_saturation(hsl.s as f32);
    s.set_color_lightness(hsl.l as f32);
    s.set_color_sat_from(color(from_hsl(Hsl { s: 0.0, ..hsl })));
    s.set_color_sat_to(color(from_hsl(Hsl { s: 100.0, ..hsl })));
    s.set_color_light_mid(color(from_hsl(Hsl { l: 50.0, ..hsl })));
}

pub(super) fn render_screen(c: &mut Controller, s: &SettingsState, state: &Value) {
    if c.model.screen(&c.stack, state, &c.strings).is_none() {
        c.stack.clear();
    }
    let Some(screen) = c.model.screen(&c.stack, state, &c.strings) else {
        return;
    };
    let screen = match &c.browser {
        Some(b) => Screen::List {
            title: b.title(&c.strings),
            rows: b.rows(&c.strings),
            dialog: None,
        },
        None => screen,
    };
    s.set_can_back(!c.stack.is_empty() || c.browser.is_some());
    let open = sessions_open(state);
    if !open || c.baseline.is_none() {
        c.baseline = Some(state["config"].clone());
    }
    let waiting = c
        .baseline
        .as_ref()
        .is_some_and(|b| pending(&state["config"], b));
    s.set_show_apply(open && waiting);
    s.set_apply_text(c.strings.t("settings.apply", "Apply").into());
    s.set_hue_label(c.strings.t("settings.colorHue", "Hue").into());
    s.set_saturation_label(c.strings.t("settings.colorSaturation", "Saturation").into());
    s.set_lightness_label(c.strings.t("settings.colorLightness", "Lightness").into());
    s.set_dialog_open(false);
    match screen {
        Screen::List {
            title,
            rows,
            dialog,
        } => {
            show_dialog(c, s, dialog);
            s.set_title(title.into());
            let mut focus_rows = Vec::new();
            let data: Vec<RowData> = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let stops = focus_stops(r);
                    let at = if stops > 0 {
                        focus_rows.len() as i32
                    } else {
                        -1
                    };
                    focus_rows.extend(std::iter::repeat_n(i as i32, stops));
                    row_data(r, at)
                })
                .collect();
            s.set_focus_rows(ModelRc::from(Rc::new(VecModel::from(focus_rows))));
            if let Some(fresh) = refill(&s.get_rows(), data) {
                s.set_rows(fresh);
            }
            c.rows = rows;
            s.set_kind(ScreenKind::List);
        }
        Screen::Select {
            title,
            options,
            read_only,
        } => {
            s.set_title(title.into());
            s.set_read_only(read_only);
            s.set_picks(ModelRc::from(Rc::new(VecModel::from(
                options
                    .into_iter()
                    .map(|p| PickData {
                        label: p.label.into(),
                        selected: p.selected,
                    })
                    .collect::<Vec<_>>(),
            ))));
            s.set_kind(ScreenKind::Select);
        }
        Screen::Number {
            title,
            value,
            min,
            max,
            step,
            text,
        } => {
            s.set_title(title.into());
            s.set_number_value(value as f32);
            s.set_number_has_min(min.is_some());
            s.set_number_min(min.unwrap_or(0.0) as f32);
            s.set_number_has_max(max.is_some());
            s.set_number_max(max.unwrap_or(0.0) as f32);
            s.set_number_step(step as f32);
            s.set_number_text(text.into());
            s.set_number_raw(number_text(value).into());
            s.set_kind(ScreenKind::Number);
        }
        Screen::Text {
            title,
            value,
            max_len,
            ..
        } => {
            s.set_title(title.into());
            s.set_text_value(value.into());
            s.set_text_max(max_len.unwrap_or(0) as i32);
            s.set_kind(ScreenKind::Text);
        }
        Screen::Color {
            title,
            value,
            custom,
        } => {
            s.set_title(title.into());
            if c.hsl.is_none() {
                s.set_color_hex_invalid(false);
            }
            let hsl = c
                .hsl
                .filter(|h| rgb(&value).is_some_and(|v| from_hsl(*h) == v))
                .unwrap_or_else(|| {
                    rgb(&value).map(to_hsl).unwrap_or(Hsl {
                        h: 0.0,
                        s: 0.0,
                        l: 0.0,
                    })
                });
            c.hsl = Some(hsl);
            show_color(s, hsl, rgb(&value).unwrap_or_else(|| from_hsl(hsl)), custom);
            s.set_kind(ScreenKind::Color);
        }
        Screen::Slider { title, level, text } => {
            s.set_title(title.into());
            s.set_level(level);
            s.set_level_text(text.into());
            s.set_kind(ScreenKind::Slider);
        }
        Screen::Calibration {
            title,
            pattern,
            knobs,
        } => {
            s.set_title(title.into());
            s.set_cal_wedge(pattern == Pattern::GrayWedge);
            s.set_reset_icon(icon("RestartAltOutlined").unwrap_or_default());
            let knobs: Vec<KnobData> = knobs
                .iter()
                .map(|k| KnobData {
                    tint: k.tint.map(color).unwrap_or_default(),
                    has_tint: k.tint.is_some(),
                    icon: k.icon.and_then(icon).unwrap_or_default(),
                    has_icon: k.icon.is_some(),
                    level: k.level(),
                    text: k.text().into(),
                    neutral: k.neutral(),
                })
                .collect();
            if let Some(fresh) = refill(&s.get_cal_knobs(), knobs) {
                s.set_cal_knobs(fresh);
            }
            s.set_kind(ScreenKind::Calibration);
        }
        Screen::Custom { title, page } => {
            s.set_title(title.into());
            s.set_pending_name(custom_name(page).into());
            s.set_kind(ScreenKind::Pending);
        }
    }
}
