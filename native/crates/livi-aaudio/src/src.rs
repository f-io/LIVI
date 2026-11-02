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
    const NAME: &'static str = "LiviAAudioSrc";
    type Type = AAudioSrc;
    type ParentType = gst_audio::AudioSrc;
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
                "AAudio Input",
                "Source/Audio",
                "Input through Android's AAudio",
                "Lasse Heitgres",
            )
        });
        Some(&*METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static TEMPLATES: LazyLock<Vec<gst::PadTemplate>> =
            LazyLock::new(|| endpoint::pad_templates("src", gst::PadDirection::Src));
        TEMPLATES.as_ref()
    }
}

impl BaseSrcImpl for Imp {}

impl PushSrcImpl for Imp {}

impl AudioBaseSrcImpl for Imp {}

impl AudioSrcImpl for Imp {
    fn prepare(&self, spec: &mut gst_audio::AudioRingBufferSpec) -> Result<(), gst::LoggableError> {
        self.0.prepare(Direction::Input, spec)
    }

    fn unprepare(&self) -> Result<(), gst::LoggableError> {
        self.0.unprepare()
    }

    fn read(
        &self,
        audio_data: &mut [u8],
    ) -> Result<(u32, Option<gst::ClockTime>), gst::LoggableError> {
        let read = self.0.transfer(Direction::Input, |s| s.read(audio_data))?;
        let read = u32::try_from(read).map_err(|e| gst::loggable_error!(CAT, "read: {e}"))?;
        Ok((read, None))
    }

    fn delay(&self) -> u32 {
        self.0.delay()
    }

    fn reset(&self) {
        self.0.reset();
    }
}

glib::wrapper! {
    pub struct AAudioSrc(ObjectSubclass<Imp>)
        @extends gst_audio::AudioSrc, gst_audio::AudioBaseSrc, gst_base::PushSrc, gst_base::BaseSrc, gst::Element, gst::Object;
}
