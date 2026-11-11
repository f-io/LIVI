// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg_attr(target_os = "android", path = "audio/audio_devices/android.rs")]
pub mod audio_devices;
pub mod host_output;
pub mod spectrum;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod system_volume;
