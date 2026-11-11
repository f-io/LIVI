// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::ptr;
use std::sync::{Arc, LazyLock};

use gstreamer as gst;
use gstreamer_audio as gst_audio;
use ndk_sys as ffi;

use crate::devices::resolve;

const UNSPECIFIED: i32 = 0;
const TIMEOUT_NS: i64 = 1_000_000_000;

pub(crate) static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new("aaudio", gst::DebugColorFlags::empty(), Some("AAudio"))
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Output,
    Input,
}

pub(crate) enum Failure {
    Disconnected,
    Other(gst::LoggableError),
}

impl From<Failure> for gst::LoggableError {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Disconnected => gst::loggable_error!(CAT, "device disconnected"),
            Failure::Other(e) => e,
        }
    }
}

pub(crate) struct Stream {
    inner: *mut ffi::AAudioStream,
    direction: Direction,
    bpf: usize,
}

// SAFETY: an AAudioStream is not tied to the thread that opened it.
unsafe impl Send for Stream {}
// SAFETY: AAudio allows control calls from any thread; the element reads or writes from one.
unsafe impl Sync for Stream {}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: inner is the open stream this value owns, closed once here.
        unsafe {
            ffi::AAudioStream_requestStop(self.inner);
            ffi::AAudioStream_close(self.inner);
        }
    }
}

struct Builder(*mut ffi::AAudioStreamBuilder);

impl Drop for Builder {
    fn drop(&mut self) {
        // SAFETY: the builder is owned here and deleted once.
        unsafe {
            ffi::AAudioStreamBuilder_delete(self.0);
        }
    }
}

fn failed(what: &str, e: impl std::fmt::Debug) -> gst::LoggableError {
    gst::loggable_error!(CAT, "{what}: {e:?}")
}

fn text(result: ffi::aaudio_result_t) -> String {
    // SAFETY: AAudio returns a static nul-terminated string for every result.
    let name = unsafe { std::ffi::CStr::from_ptr(ffi::AAudio_convertResultToText(result)) };
    name.to_string_lossy().into_owned()
}

fn checked(what: &str, result: ffi::aaudio_result_t) -> Result<(), gst::LoggableError> {
    if result < 0 {
        return Err(failed(what, text(result)));
    }
    Ok(())
}

impl Stream {
    pub(crate) fn open(
        direction: Direction,
        device: &str,
        info: &gst_audio::AudioInfo,
    ) -> Result<Arc<Self>, gst::LoggableError> {
        let format = match info.format() {
            gst_audio::AudioFormat::S16le => ffi::AAUDIO_FORMAT_PCM_I16,
            gst_audio::AudioFormat::F32le => ffi::AAUDIO_FORMAT_PCM_FLOAT,
            other => return Err(failed("unsupported format", other)),
        };
        let sink = direction == Direction::Output;
        let wanted = match device {
            "" => UNSPECIFIED,
            key => resolve(key, sink).unwrap_or_else(|| {
                eprintln!("[aaudio] {key} is gone, using the default device");
                UNSPECIFIED
            }),
        };
        let rate = i32::try_from(info.rate()).map_err(|e| failed("rate", e))?;
        let channels = i32::try_from(info.channels()).map_err(|e| failed("channels", e))?;
        let mut raw = ptr::null_mut();
        // SAFETY: raw is a valid out pointer.
        checked("builder", unsafe {
            ffi::AAudio_createStreamBuilder(&mut raw)
        })?;
        let builder = Builder(raw);
        let mut inner = ptr::null_mut();
        // SAFETY: builder.0 is a live builder and inner a valid out pointer.
        unsafe {
            let side = if sink {
                ffi::AAUDIO_DIRECTION_OUTPUT
            } else {
                ffi::AAUDIO_DIRECTION_INPUT
            };
            ffi::AAudioStreamBuilder_setDirection(builder.0, side as i32);
            ffi::AAudioStreamBuilder_setDeviceId(builder.0, wanted);
            ffi::AAudioStreamBuilder_setFormat(builder.0, format);
            ffi::AAudioStreamBuilder_setSampleRate(builder.0, rate);
            ffi::AAudioStreamBuilder_setChannelCount(builder.0, channels);
            ffi::AAudioStreamBuilder_setSharingMode(
                builder.0,
                ffi::AAUDIO_SHARING_MODE_SHARED as i32,
            );
            if sink {
                ffi::AAudioStreamBuilder_setUsage(builder.0, ffi::AAUDIO_USAGE_MEDIA as i32);
                ffi::AAudioStreamBuilder_setContentType(
                    builder.0,
                    ffi::AAUDIO_CONTENT_TYPE_MUSIC as i32,
                );
            }
            checked(
                "open",
                ffi::AAudioStreamBuilder_openStream(builder.0, &mut inner),
            )?;
        }
        let stream = Self {
            inner,
            direction,
            bpf: info.bpf() as usize,
        };
        // SAFETY: inner is the stream openStream just returned.
        checked("start", unsafe { ffi::AAudioStream_requestStart(inner) })?;
        // SAFETY: inner is an open stream.
        let got = unsafe { ffi::AAudioStream_getDeviceId(inner) };
        if wanted != UNSPECIFIED && got != wanted {
            eprintln!("[aaudio] asked for device {wanted}, Android chose {got}");
        }
        eprintln!("[aaudio] {direction:?} {rate} Hz {channels} ch on device {got}");
        Ok(Arc::new(stream))
    }

    fn frames(&self, len: usize) -> Result<i32, Failure> {
        i32::try_from(len / self.bpf).map_err(|e| Failure::Other(failed("frames", e)))
    }

    fn done(&self, what: &str, result: ffi::aaudio_result_t) -> Result<usize, Failure> {
        match result {
            ffi::AAUDIO_ERROR_DISCONNECTED => Err(Failure::Disconnected),
            r if r < 0 => Err(Failure::Other(failed(what, text(r)))),
            r => Ok(r as usize * self.bpf),
        }
    }

    fn resume(&self) {
        // SAFETY: inner is an open stream for the life of self.
        let state = unsafe { ffi::AAudioStream_getState(self.inner) } as u32;
        if matches!(
            state,
            ffi::AAUDIO_STREAM_STATE_PAUSING
                | ffi::AAUDIO_STREAM_STATE_PAUSED
                | ffi::AAUDIO_STREAM_STATE_FLUSHING
                | ffi::AAUDIO_STREAM_STATE_FLUSHED
                | ffi::AAUDIO_STREAM_STATE_STOPPING
                | ffi::AAUDIO_STREAM_STATE_STOPPED
        ) {
            // SAFETY: inner is an open stream for the life of self.
            unsafe {
                ffi::AAudioStream_requestStart(self.inner);
            }
        }
    }

    pub(crate) fn write(&self, data: &[u8]) -> Result<usize, Failure> {
        self.resume();
        let frames = self.frames(data.len())?;
        // SAFETY: data is readable for frames * bpf bytes.
        let result = unsafe {
            ffi::AAudioStream_write(self.inner, data.as_ptr().cast(), frames, TIMEOUT_NS)
        };
        self.done("write", result)
    }

    pub(crate) fn read(&self, data: &mut [u8]) -> Result<usize, Failure> {
        self.resume();
        let frames = self.frames(data.len())?;
        // SAFETY: data is writable for frames * bpf bytes.
        let result = unsafe {
            ffi::AAudioStream_read(self.inner, data.as_mut_ptr().cast(), frames, TIMEOUT_NS)
        };
        self.done("read", result)
    }

    pub(crate) fn queued(&self) -> u32 {
        // SAFETY: inner is an open stream for the life of self.
        let queued = unsafe {
            ffi::AAudioStream_getFramesWritten(self.inner)
                - ffi::AAudioStream_getFramesRead(self.inner)
        };
        u32::try_from(queued.max(0)).unwrap_or(u32::MAX)
    }

    fn settle(&self, passing: u32) {
        let passing = passing as i32;
        let mut state = passing;
        while state == passing {
            // SAFETY: inner is open and state is a valid out pointer.
            let result = unsafe {
                ffi::AAudioStream_waitForStateChange(self.inner, passing, &mut state, TIMEOUT_NS)
            };
            if result < 0 {
                return;
            }
        }
    }

    pub(crate) fn drain(&self) {
        // SAFETY: inner is an open stream for the life of self.
        unsafe {
            match self.direction {
                Direction::Output => {
                    ffi::AAudioStream_requestPause(self.inner);
                    self.settle(ffi::AAUDIO_STREAM_STATE_PAUSING);
                    ffi::AAudioStream_requestFlush(self.inner);
                    self.settle(ffi::AAUDIO_STREAM_STATE_FLUSHING);
                }
                Direction::Input => {
                    ffi::AAudioStream_requestStop(self.inner);
                }
            }
        }
    }
}
