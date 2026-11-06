// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod dashboard;
pub use livi_i18n as i18n;
pub mod icons;
pub mod keys;
pub mod mask;
pub mod media;
pub mod overlay;
pub mod scancode;
pub mod settings;
pub mod shell;
pub mod touch;

#[cfg(test)]
mod testing {
    use livi_core_proto::state::State;
    use serde_json::Value;

    pub fn state(partial: Value) -> State {
        serde_json::from_value(State::filled(partial)).expect("a filled state reads")
    }
}
