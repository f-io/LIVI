// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use crate::stream::{CAT, Direction, Failure, Stream};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_audio as gst_audio;

#[derive(Default)]
struct State {
    device: String,
    info: Option<gst_audio::AudioInfo>,
    stream: Option<Arc<Stream>>,
}

#[derive(Default)]
pub(crate) struct Endpoint {
    state: Mutex<State>,
}

impl Endpoint {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn stream(&self) -> Result<Arc<Stream>, gst::LoggableError> {
        self.state()
            .stream
            .clone()
            .ok_or_else(|| gst::loggable_error!(CAT, "not prepared"))
    }

    fn reopen(&self, direction: Direction) -> Result<Arc<Stream>, gst::LoggableError> {
        let mut state = self.state();
        state.stream = None;
        let info = state
            .info
            .clone()
            .ok_or_else(|| gst::loggable_error!(CAT, "not prepared"))?;
        let stream = Stream::open(direction, &state.device, &info)?;
        state.stream = Some(stream.clone());
        Ok(stream)
    }

    pub(crate) fn set_device(&self, value: &glib::Value) {
        self.state().device = value
            .get::<Option<String>>()
            .ok()
            .flatten()
            .unwrap_or_default();
    }

    pub(crate) fn device(&self) -> glib::Value {
        self.state().device.to_value()
    }

    pub(crate) fn prepare(
        &self,
        direction: Direction,
        spec: &gst_audio::AudioRingBufferSpec,
    ) -> Result<(), gst::LoggableError> {
        self.state().info = Some(spec.audio_info());
        self.reopen(direction).map(drop)
    }

    pub(crate) fn unprepare(&self) -> Result<(), gst::LoggableError> {
        let mut state = self.state();
        state.stream = None;
        state.info = None;
        Ok(())
    }

    pub(crate) fn transfer(
        &self,
        direction: Direction,
        mut io: impl FnMut(&Stream) -> Result<usize, Failure>,
    ) -> Result<usize, gst::LoggableError> {
        let stream = self.stream()?;
        match io(&stream) {
            Err(Failure::Disconnected) => {
                eprintln!("[aaudio] {direction:?} device disconnected, reopening");
                let stream = self.reopen(direction)?;
                Ok(io(&stream)?)
            }
            done => Ok(done?),
        }
    }

    pub(crate) fn delay(&self) -> u32 {
        self.stream().map_or(0, |s| s.queued())
    }

    pub(crate) fn reset(&self) {
        if let Ok(stream) = self.stream() {
            stream.drain();
        }
    }
}

pub(crate) fn properties() -> &'static [glib::ParamSpec] {
    static PROPERTIES: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
        vec![
            glib::ParamSpecString::builder("device")
                .nick("Device")
                .blurb("Android audio device, empty for the default")
                .build(),
        ]
    });
    PROPERTIES.as_ref()
}

pub(crate) fn pad_templates(name: &str, direction: gst::PadDirection) -> Vec<gst::PadTemplate> {
    let caps = gst_audio::AudioCapsBuilder::new_interleaved()
        .format_list([gst_audio::AudioFormat::S16le, gst_audio::AudioFormat::F32le])
        .rate_range(8000..=192_000)
        .channels_range(1..=8)
        .build();
    gst::PadTemplate::new(name, direction, gst::PadPresence::Always, &caps)
        .map(|t| vec![t])
        .unwrap_or_default()
}
