// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use gstreamer as gst;
use gstreamer_video as gst_video;
use std::path::Path;

/// Writes `sample`'s frame, in whatever raw format the decoder gave it, as an RGB PNG at `path`.
pub fn write(sample: &gst::Sample, path: &Path) -> std::io::Result<()> {
    let (w, h, stride, rgb) = rgb(sample).ok_or(std::io::ErrorKind::InvalidData)?;
    let map = rgb.map_readable().map_err(std::io::Error::other)?;
    write_png(path, w, h, stride, map.as_slice())
}

/// `sample`'s frame converted to RGB: its width, height, row stride and pixels.
fn rgb(sample: &gst::Sample) -> Option<(u32, u32, usize, gst::Buffer)> {
    let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
    let frame = gst_video::VideoFrame::from_buffer_readable(sample.buffer_owned()?, &info).ok()?;
    let out =
        gst_video::VideoInfo::builder(gst_video::VideoFormat::Rgb, info.width(), info.height())
            .fps(info.fps())
            .interlace_mode(info.interlace_mode())
            .build()
            .ok()?;
    let mut dest =
        gst_video::VideoFrame::from_buffer_writable(gst::Buffer::with_size(out.size()).ok()?, &out)
            .ok()?;
    gst_video::VideoConverter::new(&info, &out, None)
        .ok()?
        .frame(&frame, &mut dest);
    Some((
        out.width(),
        out.height(),
        out.stride()[0] as usize,
        dest.into_buffer(),
    ))
}

fn write_png(path: &Path, w: u32, h: u32, stride: usize, rgb: &[u8]) -> std::io::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    let row = w as usize * 3;
    let mut rows = Vec::with_capacity(row * h as usize);
    for y in 0..h as usize {
        rows.extend_from_slice(
            rgb.get(y * stride..y * stride + row)
                .ok_or(std::io::ErrorKind::UnexpectedEof)?,
        );
    }
    writer
        .write_image_data(&rows)
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` NV12 frame of one colour, as a decoder hands it over.
    fn nv12(w: u32, h: u32, y: u8, u: u8, v: u8) -> gst::Sample {
        gst::init().unwrap();
        let info = gst_video::VideoInfo::builder(gst_video::VideoFormat::Nv12, w, h)
            .build()
            .unwrap();
        let mut buffer = gst::Buffer::with_size(info.size()).unwrap();
        {
            let mut frame = gst_video::VideoFrameRef::from_buffer_ref_writable(
                buffer.get_mut().unwrap(),
                &info,
            )
            .unwrap();
            frame.plane_data_mut(0).unwrap().fill(y);
            for uv in frame.plane_data_mut(1).unwrap().chunks_exact_mut(2) {
                uv.copy_from_slice(&[u, v]);
            }
        }
        gst::Sample::builder()
            .caps(&info.to_caps().unwrap())
            .buffer(&buffer)
            .build()
    }

    #[test]
    fn a_decoded_frame_becomes_rgb_rows_of_its_own_size() {
        let (w, h, stride, rgb) = rgb(&nv12(6, 4, 235, 128, 128)).unwrap();
        assert_eq!((w, h), (6, 4));
        assert!(stride >= 18);
        let map = rgb.map_readable().unwrap();
        assert!(
            map[..18].iter().all(|&c| c >= 250),
            "white came out as {:?}",
            &map[..18]
        );
    }

    #[test]
    fn the_png_holds_the_frame_s_colour() {
        let path = std::env::temp_dir().join(format!("livi-frame-png-{}.png", std::process::id()));
        write(&nv12(6, 4, 82, 90, 240), &path).unwrap();
        let mut reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
                .read_info()
                .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((info.width, info.height), (6, 4));
        let [r, g, b] = [pixels[0], pixels[1], pixels[2]];
        assert!(r > 200 && g < 40 && b < 40, "red came out as {r} {g} {b}");
    }
}
