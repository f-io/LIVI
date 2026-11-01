// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const DIGIT_EM: f64 = 0.58;
const LABEL_CLEARANCE: f64 = 1.0;
const TICKS_ALONG: f64 = 40.0;
pub const START_T: f64 = -0.08;

#[derive(Debug, Clone, PartialEq)]
pub struct Scale {
    pub max: f64,
    pub majors: usize,
    pub ticks: usize,
    pub labels: Vec<String>,
}

pub fn scale(max: f64, step: f64, max_sections: usize, label: impl Fn(f64) -> String) -> Scale {
    let step = step.max(1.0);
    let sections = ((max / step).round() as usize).clamp(1, max_sections.max(1));
    let per = ((TICKS_ALONG / sections as f64).round() as usize).max(1);
    Scale {
        max: sections as f64 * step,
        majors: sections + 1,
        ticks: sections * per + 1,
        labels: (0..=sections).map(|i| label(i as f64 * step)).collect(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub radius: f64,
    pub gap_deg: f64,
    pub arm_ticks: usize,
    pub tick_w: f64,
    pub tick_h: f64,
    pub major_h: f64,
    pub label_size: f64,
    pub mirror: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            radius: 110.0,
            gap_deg: 180.0,
            arm_ticks: 3,
            tick_w: 3.0,
            tick_h: 14.0,
            major_h: 26.0,
            label_size: 15.0,
            mirror: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Minor,
    Major,
    Cap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub length: f64,
    pub width: f64,
    pub mark: Mark,
    pub arc_index: Option<usize>,
    pub opacity: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub x: f64,
    pub y: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub bars: Vec<Bar>,
    pub trail: Vec<Bar>,
    pub labels: Vec<Label>,
    pub label_size: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
    pub fit: f64,
    pub arc_ticks: usize,
}

fn rad(d: f64) -> f64 {
    d.to_radians()
}

fn arc_at(spec: &Spec, t: f64) -> (f64, f64, f64) {
    let a = spec.gap_deg / 2.0 + t * (360.0 - spec.gap_deg);
    (spec.radius * rad(a).cos(), spec.radius * rad(a).sin(), a)
}

fn arm_pitch(spec: &Spec, arc_ticks: usize) -> f64 {
    ((360.0 - spec.gap_deg) / 360.0) * 2.0 * std::f64::consts::PI * spec.radius
        / (arc_ticks.max(2) - 1) as f64
}

struct Frame {
    cx: f64,
    hw: f64,
    hh: f64,
    label_reach: f64,
}

fn frame(spec: &Spec, arc_ticks: usize) -> Frame {
    let content_right = spec.radius * rad(spec.gap_deg / 2.0).cos()
        + spec.arm_ticks as f64 * arm_pitch(spec, arc_ticks);
    let label_reach = spec.label_size + 26.0;
    Frame {
        cx: (content_right - spec.radius) / 2.0,
        hw: (content_right + spec.radius) / 2.0 + label_reach,
        hh: spec.radius + label_reach,
        label_reach,
    }
}

fn label_at(spec: &Spec, t: f64) -> (f64, f64) {
    let (x, y, a) = arc_at(spec, t);
    let off = spec.major_h / 2.0 + spec.label_size * 0.9;
    (x + rad(a).cos() * off, y + rad(a).sin() * off)
}

pub fn label_overflow(spec: &Spec, scale: &Scale, box_w: f64, box_h: f64, px: f64) -> f64 {
    if scale.majors < 2 {
        return 0.0;
    }
    let f = frame(spec, scale.ticks);
    let fit = (box_w / (f.hw * 2.0)).min(box_h / (f.hh * 2.0));
    let margin = (box_w - f.hw * 2.0 * fit) / 2.0;
    let screen_edge = spec.radius + f.label_reach - (px - margin) / fit;
    let reach = (0..scale.majors)
        .map(|m| {
            let (x, _) = label_at(spec, m as f64 / (scale.majors - 1) as f64);
            let half = scale.labels.get(m).map_or(0, |l| l.chars().count()) as f64
                * DIGIT_EM
                * spec.label_size
                / 2.0;
            half - x
        })
        .fold(0.0, f64::max);
    ((reach + LABEL_CLEARANCE - screen_edge) * fit).max(0.0)
}

fn majors_on_arc(majors: usize, arc_ticks: usize) -> Vec<usize> {
    if majors < 2 {
        return Vec::new();
    }
    (0..majors)
        .map(|m| ((m as f64 / (majors - 1) as f64) * (arc_ticks - 1) as f64).round() as usize)
        .collect()
}

pub fn draw(spec: &Spec, scale: &Scale, box_w: f64, box_h: f64) -> Drawing {
    let arc_ticks = scale.ticks.max(2);
    let caps = spec.arm_ticks;
    let f = frame(spec, arc_ticks);
    let sx = if spec.mirror { -1.0 } else { 1.0 };
    let (vx, vy) = (sx * f.cx - f.hw, -f.hh);
    let fit = (box_w / (f.hw * 2.0)).min(box_h / (f.hh * 2.0));
    let (ox, oy) = (
        (box_w - f.hw * 2.0 * fit) / 2.0,
        (box_h - f.hh * 2.0 * fit) / 2.0,
    );
    let to_box = |x: f64, y: f64| (ox + (x - vx) * fit, oy + (y - vy) * fit);
    let place = |x: f64, y: f64, ang: f64| {
        let (bx, by) = to_box(x * sx, y);
        (bx, by, if spec.mirror { 180.0 - ang } else { ang })
    };
    let majors = majors_on_arc(scale.majors, arc_ticks);
    let g = spec.gap_deg / 2.0;
    let x_end = spec.radius * rad(g).cos();
    let y_lo = spec.radius * rad(g).sin();
    let pitch = arm_pitch(spec, arc_ticks);
    let cap_fade = |k: usize| {
        if caps <= 1 {
            1.0
        } else {
            0.18 + 0.72 * ((caps - k) as f64 / (caps - 1) as f64)
        }
    };
    let bar = |x: f64, y: f64, ang: f64, mark: Mark, arc_index: Option<usize>, opacity: f64| {
        let h = if mark == Mark::Major {
            spec.major_h
        } else {
            spec.tick_h
        };
        let (px, py, pang) = place(x, y, ang);
        let shift = (h - spec.tick_h) / 2.0 * fit;
        Bar {
            x: px - rad(pang).cos() * shift,
            y: py - rad(pang).sin() * shift,
            angle: pang + 90.0,
            length: h * fit,
            width: spec.tick_w * fit,
            mark,
            arc_index,
            opacity,
        }
    };
    let mut bars = Vec::new();
    let mut trail = Vec::new();
    for k in (1..=caps).rev() {
        bars.push(bar(
            x_end + k as f64 * pitch,
            y_lo,
            90.0,
            Mark::Cap,
            None,
            cap_fade(k),
        ));
    }
    for i in 0..arc_ticks {
        let (x, y, a) = arc_at(spec, i as f64 / (arc_ticks - 1) as f64);
        let mark = if majors.contains(&i) {
            Mark::Major
        } else {
            Mark::Minor
        };
        bars.push(bar(x, y, a, mark, Some(i), 1.0));
        trail.push(bar(x, y, a, Mark::Minor, Some(i), 1.0));
    }
    for k in 1..=caps {
        bars.push(bar(
            x_end + k as f64 * pitch,
            -y_lo,
            270.0,
            Mark::Cap,
            None,
            cap_fade(k),
        ));
    }
    let labels = (0..scale.majors.max(2))
        .filter(|_| scale.majors >= 2)
        .map(|m| {
            let (x, y) = label_at(spec, m as f64 / (scale.majors - 1) as f64);
            let (bx, by) = to_box(x * sx, y);
            Label {
                x: bx,
                y: by,
                text: scale.labels.get(m).cloned().unwrap_or_default(),
            }
        })
        .collect();
    let (center_x, center_y) = to_box(0.0, 0.0);
    Drawing {
        bars,
        trail,
        labels,
        label_size: spec.label_size * fit,
        center_x,
        center_y,
        radius: spec.radius * fit,
        fit,
        arc_ticks,
    }
}

impl Bar {
    pub fn path(&self) -> String {
        let (s, c) = rad(self.angle).sin_cos();
        let (hw, hl) = (self.width / 2.0, self.length / 2.0);
        let corner = |lx: f64, ly: f64| {
            format!(
                "{:.2} {:.2}",
                self.x + lx * c - ly * s,
                self.y + lx * s + ly * c
            )
        };
        format!(
            "M {} L {} L {} L {} Z",
            corner(-hw, -hl),
            corner(hw, -hl),
            corner(hw, hl),
            corner(-hw, hl)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Groups {
    pub minor: String,
    pub major: String,
    pub red_minor: String,
    pub red_major: String,
    pub caps: Vec<(String, f64)>,
}

pub fn groups(d: &Drawing, redline_index: Option<f64>) -> Groups {
    let mut g = Groups::default();
    let join = |acc: &mut String, b: &Bar| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(&b.path());
    };
    for b in &d.bars {
        match (b.arc_index, b.mark) {
            (None, _) => {
                match g
                    .caps
                    .iter_mut()
                    .find(|(_, o)| (*o - b.opacity).abs() < 1e-6)
                {
                    Some((p, _)) => join(p, b),
                    None => g.caps.push((b.path(), b.opacity)),
                }
            }
            (Some(i), mark) => {
                let red = redline_index.is_some_and(|r| i as f64 >= r);
                let target = match (red, mark == Mark::Major) {
                    (false, false) => &mut g.minor,
                    (false, true) => &mut g.major,
                    (true, false) => &mut g.red_minor,
                    (true, true) => &mut g.red_major,
                };
                join(target, b);
            }
        }
    }
    g
}

const SHADOW_GROW: f64 = 6.0;
const SHADOW_WIDTH: f64 = 42.0;
const SHADOW_BLUR: f64 = 13.0;

pub struct Shadow {
    pub size: usize,
    pub half: f64,
    pub alpha: Vec<u8>,
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let p = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let y = 1.0 - p * (-x * x).exp();
    if x < 0.0 { -y } else { y }
}

fn blurred_stroke(d: f64) -> f64 {
    let s = SHADOW_BLUR * std::f64::consts::SQRT_2;
    let w = SHADOW_WIDTH / 2.0;
    0.5 * (erf((w - d) / s) + erf((w + d) / s))
}

fn smoothstep(edge: f64, x: f64) -> f64 {
    let t = (x / edge).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn shadow(d: &Drawing, mirror: bool) -> Shadow {
    let r = d.radius + SHADOW_GROW;
    let arm = d
        .bars
        .iter()
        .filter(|b| b.arc_index.is_none())
        .map(|b| (b.x - d.center_x).abs() + b.width / 2.0)
        .fold(0.0, f64::max);
    let half = r + SHADOW_WIDTH / 2.0 + 3.0 * SHADOW_BLUR;
    let size = (2.0 * half).ceil() as usize;
    let mut alpha = Vec::with_capacity(size * size);
    for row in 0..size {
        for col in 0..size {
            let x = col as f64 + 0.5 - half;
            let y = row as f64 + 0.5 - half;
            let outward = if mirror { x } else { -x };
            let a = if outward >= 0.0 {
                blurred_stroke((x.hypot(y) - r).abs())
            } else {
                blurred_stroke((y.abs() - r).abs())
                    * (1.0 - smoothstep(arm + SHADOW_BLUR, -outward))
            };
            alpha.push((a * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    Shadow { size, half, alpha }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shadow_follows_the_outer_arc_and_runs_out_along_the_arms() {
        let sc = scale(200.0, 20.0, 10, |v| format!("{v}"));
        let d = draw(&Spec::default(), &sc, 470.0, 600.0);
        let s = shadow(&d, false);
        let r = d.radius + 6.0;
        let at = |x: f64, y: f64| {
            let col = (s.half + x) as usize;
            let row = (s.half + y) as usize;
            s.alpha[row * s.size + col]
        };
        assert!(at(-r, 0.0) > 200);
        assert_eq!(at(r, 0.0), 0);
        assert_eq!(at(0.0, 0.0), 0);
        assert!(at(0.0, r) > 200);
        assert!(at(8.0, -r) > 150);
        assert!(at(8.0, -r) < at(0.0, -r));
        let m = shadow(&d, true);
        assert_eq!(m.alpha[(m.size / 2) * m.size + 2], 0);
    }

    #[test]
    fn a_bar_is_a_closed_quad_around_its_centre() {
        let b = Bar {
            x: 10.0,
            y: 20.0,
            angle: 90.0,
            length: 4.0,
            width: 2.0,
            mark: Mark::Minor,
            arc_index: Some(0),
            opacity: 1.0,
        };
        assert_eq!(
            b.path(),
            "M 12.00 19.00 L 12.00 21.00 L 8.00 21.00 L 8.00 19.00 Z"
        );
    }

    #[test]
    fn the_red_zone_gets_its_own_paths() {
        let s = scale(8000.0, 1000.0, 20, |v| format!("{v}"));
        let d = draw(&Spec::default(), &s, 470.0, 600.0);
        let none = groups(&d, None);
        assert!(none.red_minor.is_empty() && none.red_major.is_empty());
        let red = groups(&d, Some((s.ticks - 1) as f64 * 0.9));
        assert!(!red.red_minor.is_empty() && !red.red_major.is_empty());
        assert_eq!(red.caps.len(), 3);
    }

    #[test]
    fn a_scale_ends_on_a_whole_step() {
        let s = scale(250.0, 20.0, 15, |v| format!("{v}"));
        assert_eq!(s.max, 260.0);
        assert_eq!(s.majors, 14);
        assert_eq!(s.labels.first().map(String::as_str), Some("0"));
        assert_eq!(s.labels.last().map(String::as_str), Some("260"));
        assert_eq!(s.ticks, 13 * 3 + 1);
        let capped = scale(500.0, 10.0, 15, |v| format!("{v}"));
        assert_eq!(capped.majors, 16);
    }

    #[test]
    fn the_arc_runs_bottom_to_top_with_caps_around_it() {
        let s = scale(8000.0, 1000.0, 20, |v| format!("{}", v / 1000.0));
        let d = draw(&Spec::default(), &s, 470.0, 600.0);
        let arc: Vec<&Bar> = d.bars.iter().filter(|b| b.arc_index.is_some()).collect();
        assert_eq!(arc.len(), s.ticks);
        assert_eq!(d.bars.len(), s.ticks + 6);
        assert!(arc[0].y > d.center_y, "0 sits at the bottom");
        assert!(arc[arc.len() - 1].y < d.center_y, "the end sits at the top");
        assert!(
            arc[arc.len() / 2].x < d.center_x,
            "the round side points left"
        );
        assert_eq!(
            arc.iter().filter(|b| b.mark == Mark::Major).count(),
            s.majors
        );
        assert_eq!(d.labels.len(), s.majors);
    }

    #[test]
    fn a_mirrored_gauge_opens_the_other_way() {
        let s = scale(8000.0, 1000.0, 20, |v| format!("{v}"));
        let spec = Spec {
            mirror: true,
            ..Spec::default()
        };
        let d = draw(&spec, &s, 470.0, 600.0);
        let mid = d
            .bars
            .iter()
            .filter(|b| b.arc_index.is_some())
            .nth(s.ticks / 2);
        assert!(mid.unwrap().x > d.center_x);
    }

    #[test]
    fn long_numbers_push_the_gauge_in() {
        let narrow = scale(100.0, 20.0, 15, |v| format!("{v}"));
        let wide = scale(400.0, 20.0, 15, |v| format!("{v}"));
        let spec = Spec::default();
        assert!(
            label_overflow(&spec, &wide, 470.0, 600.0, 60.0)
                >= label_overflow(&spec, &narrow, 470.0, 600.0, 60.0)
        );
    }
}
