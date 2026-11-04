// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::f32::consts::PI;

use crate::MPX_RATE;

const PILOT_HZ: f32 = 19_000.0;
const ARM_HZ: f32 = 300.0;
const LOOP_HZ: f32 = 20.0;
const PILOT_MIN: f32 = 0.04;
const LOCKED_PHASE: f32 = 0.2;

/// Locks to the 19 kHz pilot and turns the multiplex into the L−R difference signal on the
/// 38 kHz subcarrier, before any filtering.
pub struct Pilot {
    theta: f32,
    freq: f32,
    i: f32,
    q: f32,
    err: f32,
    alpha: f32,
    kp: f32,
    ki: f32,
}

impl Pilot {
    pub fn new() -> Self {
        let rate = MPX_RATE as f32;
        let wn = 2.0 * PI * LOOP_HZ / rate;
        Self {
            theta: 0.0,
            freq: 2.0 * PI * PILOT_HZ / rate,
            i: 0.0,
            q: 0.0,
            err: 1.0,
            alpha: 1.0 - (-2.0 * PI * ARM_HZ / rate).exp(),
            kp: 2.0 * 0.707 * wn,
            ki: wn * wn,
        }
    }

    /// The pilot's amplitude in multiplex units, nominally 0.1.
    pub fn amplitude(&self) -> f32 {
        2.0 * (self.i * self.i + self.q * self.q).sqrt()
    }

    pub fn locked(&self) -> bool {
        self.amplitude() > PILOT_MIN && self.err < LOCKED_PHASE
    }

    pub fn difference(&mut self, mpx: &[f32], out: &mut Vec<f32>) {
        for &x in mpx {
            let (s, c) = self.theta.sin_cos();
            self.i += self.alpha * (x * c - self.i);
            self.q += self.alpha * (-x * s - self.q);
            let e = self.q.atan2(self.i);
            self.err += 0.001 * (e.abs() - self.err);
            self.freq += self.ki * e;
            self.theta += self.freq + self.kp * e;
            if self.theta > PI {
                self.theta -= 2.0 * PI;
            }
            // The pilot is cos θ here; the standard's pilot sin ψ puts the subcarrier sin 2ψ at
            // −sin 2θ.
            out.push(-4.0 * x * s * c);
        }
    }
}
