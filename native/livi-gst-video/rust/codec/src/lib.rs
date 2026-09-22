//! Element choice per codec: parser, decoder candidates and appsrc caps.
//!
//! The candidate lists are ordered best first. The caller takes the first name
//! the GStreamer registry knows.

use core::ffi::CStr;

const H264: &CStr = c"h264";
const H265: &CStr = c"h265";
const VP9: &CStr = c"vp9";
const AV1: &CStr = c"av1";

/// The parser element for `codec`, h264 for anything unknown.
pub fn parser_for(codec: &str) -> &'static CStr {
    match codec {
        "h265" => c"h265parse",
        "vp9" => c"vp9parse",
        "av1" => c"av1parse",
        _ => c"h264parse",
    }
}

/// The appsrc caps for `codec`, h264 for anything unknown.
pub fn caps_for(codec: &str) -> &'static CStr {
    match codec {
        "h265" => c"video/x-h265,stream-format=byte-stream",
        "vp9" => c"video/x-vp9",
        "av1" => c"video/x-av1",
        _ => c"video/x-h264,stream-format=byte-stream",
    }
}

/// The software decoder for `codec`, used to report software availability and
/// as the only candidate under LIVI_GST_SWDEC.
pub fn sw_decoder_for(codec: &str) -> &'static CStr {
    match codec {
        "h265" => c"avdec_h265",
        "vp9" => c"vp9dec",
        "av1" => c"dav1ddec",
        _ => c"avdec_h264",
    }
}

/// True for every decoder that is not one of the known software ones.
pub fn is_hw_decoder(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with("avdec_") {
        return false;
    }
    !matches!(name, "vp9dec" | "vp8dec" | "dav1ddec" | "openh264dec")
}

/// Decoder candidates for `codec`, hardware first. With `sw_only` the software
/// decoder is the only candidate.
pub fn decoder_candidates(codec: &str, sw_only: bool) -> &'static [&'static CStr] {
    if sw_only {
        return match codec {
            "h265" => &[c"avdec_h265"],
            "vp9" => &[c"vp9dec"],
            "av1" => &[c"dav1ddec"],
            _ => &[c"avdec_h264"],
        };
    }
    #[cfg(target_os = "macos")]
    {
        // The bundled vtdec carries scripts/gstreamer/patches/gst-plugins-bad/0007,
        // so HEVC decodes low-latency in hardware.
        // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/work_items/5133
        match codec {
            "h265" => &[c"vtdec", c"avdec_h265"],
            "vp9" => &[c"vp9dec"],
            "av1" => &[c"dav1ddec"],
            _ => &[c"vtdec", c"avdec_h264"],
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match codec {
            "h265" => &[c"v4l2slh265dec", c"v4l2h265dec", c"vah265dec", c"avdec_h265"],
            "vp9" => &[c"v4l2slvp9dec", c"v4l2vp9dec", c"vavp9dec", c"vp9dec"],
            "av1" => &[c"vaav1dec", c"dav1ddec"],
            _ => &[c"v4l2slh264dec", c"v4l2h264dec", c"vah264dec", c"avdec_h264"],
        }
    }
}

/// Codec names the caller may pass. Anything else is read as h264.
pub fn known_codecs() -> [&'static CStr; 4] {
    [H264, H265, VP9, AV1]
}

/// The sink chain for this platform. `LIVI_GST_SINK` replaces the element name
/// on Linux, where the decoded dmabuf goes to livi-compositor zero-copy.
pub fn sink_chain(sink_override: Option<&str>) -> String {
    #[cfg(target_os = "macos")]
    {
        let _ = sink_override;
        // force-aspect-ratio=false: the clip view enforces the ratio, the sink must fill
        "glimagesink name=sink sync=false qos=false force-aspect-ratio=false".to_owned()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let name = sink_override.filter(|s| !s.is_empty()).unwrap_or("waylandsink");
        format!("{name} name=sink sync=false")
    }
}

/// The element before the sink: software decoders hand out plain frames that
/// still need converting.
pub fn presink(decoder: &str) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        let _ = decoder;
        ""
    }
    #[cfg(not(target_os = "macos"))]
    {
        if is_hw_decoder(decoder) { "" } else { "videoconvert ! " }
    }
}

/// The colour-calibration pass, empty where the platform has none.
pub fn calibration() -> &'static str {
    if cfg!(target_os = "macos") { "glupload ! glcolorconvert ! glshader name=cal ! " } else { "" }
}

/// The whole pipeline for `codec` through `decoder`. The queue ahead of the
/// decoder keeps every frame for the decoder's reference chain, the one behind
/// it drops when the sink falls behind.
pub fn pipeline_desc(codec: &str, decoder: &str, cal: &str, sink_override: Option<&str>) -> String {
    format!(
        "appsrc name=src is-live=true do-timestamp=true format=time \
         min-latency=0 max-latency=0 caps={caps} \
         ! {parser} \
         ! queue max-size-buffers=0 max-size-bytes=0 max-size-time=2000000000 \
         ! {decoder} name=dec \
         ! queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream \
         ! {presink}{cal}{sink}",
        caps = caps_for(codec).to_str().unwrap_or(""),
        parser = parser_for(codec).to_str().unwrap_or(""),
        presink = presink(decoder),
        sink = sink_chain(sink_override),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_codec_gets_its_parser_and_caps() {
        assert_eq!(parser_for("h265"), c"h265parse");
        assert_eq!(parser_for("vp9"), c"vp9parse");
        assert_eq!(parser_for("av1"), c"av1parse");
        assert_eq!(caps_for("h265"), c"video/x-h265,stream-format=byte-stream");
        assert_eq!(caps_for("vp9"), c"video/x-vp9");
        assert_eq!(caps_for("av1"), c"video/x-av1");
    }

    #[test]
    fn anything_unknown_is_read_as_h264() {
        for codec in ["h264", "", "H265", "mpeg2", "hevc"] {
            assert_eq!(parser_for(codec), c"h264parse", "{codec}");
            assert_eq!(caps_for(codec), c"video/x-h264,stream-format=byte-stream", "{codec}");
            assert_eq!(sw_decoder_for(codec), c"avdec_h264", "{codec}");
        }
    }

    #[test]
    fn the_software_decoders_are_named_per_codec() {
        assert_eq!(sw_decoder_for("h265"), c"avdec_h265");
        assert_eq!(sw_decoder_for("vp9"), c"vp9dec");
        assert_eq!(sw_decoder_for("av1"), c"dav1ddec");
    }

    #[test]
    fn the_known_software_decoders_are_not_hardware() {
        for name in ["avdec_h264", "avdec_h265", "vp9dec", "vp8dec", "dav1ddec", "openh264dec"] {
            assert!(!is_hw_decoder(name), "{name}");
        }
    }

    #[test]
    fn everything_else_counts_as_hardware() {
        for name in ["vtdec", "v4l2slh265dec", "v4l2h264dec", "vah264dec"] {
            assert!(is_hw_decoder(name), "{name}");
        }
        assert!(!is_hw_decoder(""));
    }

    #[test]
    fn sw_only_leaves_one_candidate_and_it_is_the_software_one() {
        for codec in ["h264", "h265", "vp9", "av1"] {
            let names = decoder_candidates(codec, true);
            assert_eq!(names.len(), 1, "{codec}");
            assert_eq!(names[0], sw_decoder_for(codec), "{codec}");
            assert!(!is_hw_decoder(names[0].to_str().unwrap()), "{codec}");
        }
    }

    #[test]
    fn every_candidate_list_holds_a_software_fallback() {
        for codec in ["h264", "h265", "vp9", "av1"] {
            let names = decoder_candidates(codec, false);
            assert!(!names.is_empty(), "{codec}");
            assert!(
                names.iter().any(|n| !is_hw_decoder(n.to_str().unwrap())),
                "{codec} offers no software decoder"
            );
        }
    }

    #[test]
    fn candidates_carry_no_duplicates() {
        for codec in ["h264", "h265", "vp9", "av1"] {
            let names = decoder_candidates(codec, false);
            for (i, a) in names.iter().enumerate() {
                assert!(!names[i + 1..].contains(a), "{codec} repeats {a:?}");
            }
        }
    }

    #[test]
    fn the_known_codec_names_are_the_ones_the_tables_answer_to() {
        for codec in known_codecs() {
            let name = codec.to_str().unwrap();
            assert_eq!(decoder_candidates(name, true)[0], sw_decoder_for(name));
        }
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;

    #[test]
    fn the_description_names_source_parser_decoder_and_sink() {
        let d = pipeline_desc("h265", "avdec_h265", "", None);

        assert!(d.starts_with("appsrc name=src"));
        assert!(d.contains("caps=video/x-h265,stream-format=byte-stream"));
        assert!(d.contains("h265parse"));
        assert!(d.contains("avdec_h265 name=dec"));
        assert!(d.contains("name=sink"));
    }

    #[test]
    fn every_link_is_a_bang_between_two_spaces() {
        for cal in ["", "glupload ! glshader name=cal ! "] {
            let d = pipeline_desc("h264", "avdec_h264", cal, None);
            let bytes = d.as_bytes();
            for (i, c) in bytes.iter().enumerate() {
                if *c != b'!' {
                    continue;
                }
                assert_eq!(bytes.get(i - 1), Some(&b' '), "no space before ! in {d}");
                assert_eq!(bytes.get(i + 1), Some(&b' '), "no space after ! in {d}");
            }
        }
    }

    #[test]
    fn each_stage_carries_exactly_one_element() {
        let d = pipeline_desc("h265", "avdec_h265", "", None);
        let stages: Vec<&str> = d.split(" ! ").collect();

        assert!(stages.len() >= 5, "{d}");
        for stage in stages {
            assert!(!stage.is_empty(), "{d}");
            assert!(!stage.contains('!'), "unsplit stage {stage:?}");
            // a property glued to the next element reads as a word with no space before "name="
            assert!(!stage.contains("timemin"), "{stage:?}");
        }
    }

    #[test]
    fn the_queue_ahead_of_the_decoder_never_leaks() {
        let d = pipeline_desc("h264", "avdec_h264", "", None);
        let (before, after) = d.split_once("name=dec").unwrap();

        assert!(!before.contains("leaky"));
        assert!(after.contains("leaky=downstream"));
    }

    #[test]
    fn a_calibration_pass_sits_between_the_queue_and_the_sink() {
        let d = pipeline_desc("h264", "vtdec", "glupload ! glshader name=cal ! ", None);
        let i = d.find("glshader name=cal").unwrap();

        assert!(i > d.find("name=dec").unwrap());
        assert!(i < d.find("name=sink").unwrap());
    }

    #[test]
    fn an_empty_calibration_leaves_the_chain_untouched() {
        assert!(!pipeline_desc("h264", "avdec_h264", "", None).contains("glshader"));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_software_decoder_gets_a_converter_and_a_hardware_one_does_not() {
        assert_eq!(presink("avdec_h264"), "videoconvert ! ");
        assert_eq!(presink("v4l2slh264dec"), "");
        assert!(pipeline_desc("h264", "avdec_h264", "", None).contains("videoconvert ! "));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_sink_can_be_replaced_by_name() {
        assert_eq!(sink_chain(None), "waylandsink name=sink sync=false");
        assert_eq!(sink_chain(Some("")), "waylandsink name=sink sync=false");
        assert_eq!(sink_chain(Some("fakesink")), "fakesink name=sink sync=false");
        assert!(pipeline_desc("h264", "avdec_h264", "", Some("fakesink")).contains("fakesink name=sink"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_sink_fills_the_view_and_ignores_the_override() {
        assert!(sink_chain(Some("fakesink")).starts_with("glimagesink name=sink"));
        assert!(sink_chain(None).contains("force-aspect-ratio=false"));
        assert_eq!(presink("avdec_h264"), "");
    }
}

