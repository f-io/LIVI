// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

#[derive(Clone, Copy)]
pub(super) struct Offered {
    pub(super) size: (u16, u16),
    pub(super) refresh: u32,
    pub(super) preferred: bool,
}

impl Offered {
    pub(super) fn area(&self) -> u32 {
        u32::from(self.size.0) * u32::from(self.size.1)
    }

    pub(super) fn fits_hd(&self) -> bool {
        self.area() <= 1280 * 720
    }
}

pub(super) fn pick_mode(modes: &[Mode], setting: Option<&str>) -> Option<Mode> {
    let offered: Vec<Offered> = modes
        .iter()
        .map(|m| Offered {
            size: m.size(),
            refresh: m.vrefresh(),
            preferred: m.mode_type().contains(ModeTypeFlags::PREFERRED),
        })
        .collect();
    choose(&offered, setting.and_then(size_of)).map(|i| modes[i])
}

pub(super) fn size_of(mode: &str) -> Option<(u16, u16)> {
    let (w, h) = mode.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

pub(super) fn choose(modes: &[Offered], configured: Option<(u16, u16)>) -> Option<usize> {
    let largest = |fits: &dyn Fn(&Offered) -> bool| {
        modes
            .iter()
            .enumerate()
            .filter(|(_, m)| fits(m))
            .max_by_key(|(_, m)| (m.area(), m.refresh))
            .map(|(i, _)| i)
    };
    if let Some(size) = configured
        && let Some(i) = largest(&|m| m.size == size)
    {
        return Some(i);
    }
    if let Some(i) = modes.iter().position(|m| m.preferred && m.fits_hd()) {
        return Some(i);
    }
    largest(&Offered::fits_hd).or_else(|| {
        modes
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| m.area())
            .map(|(i, _)| i)
    })
}

pub(super) fn sizes(modes: &[Mode]) -> Vec<String> {
    let mut sizes: Vec<(u16, u16)> = Vec::new();
    for mode in modes {
        if !sizes.contains(&mode.size()) {
            sizes.push(mode.size());
        }
    }
    sizes.sort_by_key(|&(w, h)| std::cmp::Reverse(u32::from(w) * u32::from(h)));
    sizes.iter().map(|(w, h)| format!("{w}x{h}")).collect()
}
