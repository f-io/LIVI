// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::LazyLock;

use crate::endpoint::{self, Endpoint};
use crate::stream::{CAT, Direction};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::subclass::prelude::*;
use gstreamer_audio as gst_audio;
use gstreamer_audio::subclass::prelude::*;
use gstreamer_base as gst_base;

#[derive(Default)]
pub struct Imp(Endpoint);

#[glib::object_subclass]
impl ObjectSubclass for Imp {
    const NAME: &'static str = "LiviAAudioSink";
    type Type = AAudioSink;
    type ParentType = gst_audio::AudioSink;
}

impl ObjectImpl for Imp {
    fn properties() -> &'static [glib::ParamSpec] {
        endpoint::properties()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, _pspec: &glib::ParamSpec) {
        self.0.set_device(value);
    }

    fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value {
        self.0.device()
    }
}

impl GstObjectImpl for Imp {}

impl ElementImpl for Imp {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "AAudio Output",
                "Sink/Audio",
                "Output through Android's AAudio",
                "Lasse Heitgres",
            )
        });
        Some(&*METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static TEMPLATES: LazyLock<Vec<gst::PadTemplate>> =
            LazyLock::new(|| endpoint::pad_templates("sink", gst::PadDirection::Sink));
        TEMPLATES.as_ref()
    }
}

impl BaseSinkImpl for Imp {}

impl AudioBaseSinkImpl for Imp {}

impl AudioSinkImpl for Imp {
    fn prepare(&self, spec: &mut gst_audio::AudioRingBufferSpec) -> Result<(), gst::LoggableError> {
        self.0.prepare(Direction::Output, spec)
    }

    fn unprepare(&self) -> Result<(), gst::LoggableError> {
        self.0.unprepare()
    }

    fn write(&self, audio_data: &[u8]) -> Result<i32, gst::LoggableError> {
        let written = self
            .0
            .transfer(Direction::Output, |s| s.write(audio_data))?;
        i32::try_from(written).map_err(|e| gst::loggable_error!(CAT, "write: {e}"))
    }

    fn delay(&self) -> u32 {
        self.0.delay()
    }

    fn reset(&self) {
        self.0.reset();
    }
}

glib::wrapper! {
    pub struct AAudioSink(ObjectSubclass<Imp>)
        @extends gst_audio::AudioSink, gst_audio::AudioBaseSink, gst_base::BaseSink, gst::Element, gst::Object;
}
