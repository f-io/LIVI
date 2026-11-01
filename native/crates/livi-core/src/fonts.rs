// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use skrifa::MetadataProvider;
use skrifa::raw::FileRef;

use crate::server::Core;

struct Source {
    url: &'static str,
    sha256: &'static str,
}

struct Font {
    script: [u8; 4],
    #[cfg_attr(target_os = "macos", expect(dead_code))]
    fontconfig: &'static str,
    probe: char,
    file: &'static str,
    sources: [Source; 2],
}

const FONTS: [Font; 7] = [
    Font {
        script: *b"Hani",
        fontconfig: "zh-cn",
        probe: '中',
        file: "NotoSansSC.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf",
                sha256: "a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosanssc/v41/k3kCo84MPvpLmixcA63oeAL7Iqp5IZJF9bmaG9_FnYw.ttf",
                sha256: "450625c8d46ab3df97b7904ded955ec2746d17ec76740cb1e91d1ba63a0f89af",
            },
        ],
    },
    Font {
        script: *b"Hira",
        fontconfig: "ja",
        probe: 'あ',
        file: "NotoSansJP.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosansjp/NotoSansJP%5Bwght%5D.ttf",
                sha256: "c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosansjp/v57/-F6jfjtqLzI2JPCgQBnw7HFyzSD-AsregP8VFBEj75s.ttf",
                sha256: "f037d714109f22d57ce75fd9a6b9d23439c903061fb11106c070b2dcc03f2021",
            },
        ],
    },
    Font {
        script: *b"Hang",
        fontconfig: "ko",
        probe: '한',
        file: "NotoSansKR.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosanskr/NotoSansKR%5Bwght%5D.ttf",
                sha256: "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosanskr/v40/PbyxFmXiEBPT4ITbgNA5Cgms3VYcOA-vvnIzzuoyeLQ.ttf",
                sha256: "84e2b8acadd0a01af9f0de6b0a2d29117fb5b0eabd7c2f8353d9d2da0461d5fa",
            },
        ],
    },
    Font {
        script: *b"Arab",
        fontconfig: "ar",
        probe: 'ع',
        file: "NotoSansArabic.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosansarabic/NotoSansArabic%5Bwdth%2Cwght%5D.ttf",
                sha256: "63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosansarabic/v33/nwpxtLGrOAZMl5nJ_wfgRg3DrWFZWsnVBJ_sS6tlqHHFlhQ5l3sQWIHPqzCfyGyvuw.ttf",
                sha256: "146b2193f4aee343a8da5e2295255b04db547d74339a12be51763b3a0081868d",
            },
        ],
    },
    Font {
        script: *b"Hebr",
        fontconfig: "he",
        probe: 'א',
        file: "NotoSansHebrew.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosanshebrew/NotoSansHebrew%5Bwdth%2Cwght%5D.ttf",
                sha256: "7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosanshebrew/v50/or3HQ7v33eiDljA1IufXTtVf7V6RvEEdhQlk0LlGxCyaeNKYZC0sqk3xXGiXd4qtog.ttf",
                sha256: "a5cc5ce90ed4f6393992728fcc63657c4dd83aed119acbd5f4ed223e40c71129",
            },
        ],
    },
    Font {
        script: *b"Thai",
        fontconfig: "th",
        probe: 'ก',
        file: "NotoSansThai.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosansthai/NotoSansThai%5Bwdth%2Cwght%5D.ttf",
                sha256: "5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosansthai/v29/iJWnBXeUZi_OHPqn4wq6hQ2_hbJ1xyN9wd43SofNWcd1MKVQt_So_9CdU5RtpzE.ttf",
                sha256: "6c5e4ca047263ed8f07b174be2f7c396d245504ce0f77adc1d26b0978bc95426",
            },
        ],
    },
    Font {
        script: *b"Deva",
        fontconfig: "hi",
        probe: 'क',
        file: "NotoSansDevanagari.ttf",
        sources: [
            Source {
                url: "https://raw.githubusercontent.com/google/fonts/9b59ea9f333c5b8a99fa44e2a698496d6cde5b8e/ofl/notosansdevanagari/NotoSansDevanagari%5Bwdth%2Cwght%5D.ttf",
                sha256: "14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e",
            },
            Source {
                url: "https://fonts.gstatic.com/s/notosansdevanagari/v33/TuGoUUFzXI5FBtUq5a8bjKYTZjtRU6Sgv3NaV_SNmI0b8QQCQmHn6B2OHjbL_08AlXQly-A.ttf",
                sha256: "1ec0511798a17f5a137aa83671fded50120f79cecafa7189ad76cc55803bc4ee",
            },
        ],
    },
];
const USER_AGENT: &str = "LIVI";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

fn font_for(language: &str) -> Option<&'static Font> {
    let script = livi_i18n::script(language)?;
    FONTS.iter().find(|f| f.script == script)
}

fn is_font(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
}

fn scan(dir: &Path) -> Vec<String> {
    let mut fonts: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_font(p))
        .map(|p| p.display().to_string())
        .collect();
    fonts.sort();
    fonts
}

fn covers(path: &str, probe: char) -> bool {
    let Ok(data) = std::fs::read(path) else {
        return false;
    };
    let Ok(file) = FileRef::new(&data) else {
        return false;
    };
    file.fonts()
        .flatten()
        .any(|font| font.charmap().map(probe).is_some_and(|g| g.to_u32() != 0))
}

#[cfg(target_os = "macos")]
async fn system_has(_font: &Font) -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
async fn system_has(font: &Font) -> bool {
    tokio::process::Command::new("fc-list")
        .args([format!(":lang={}", font.fontconfig).as_str(), "family"])
        .output()
        .await
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn fetch(client: &reqwest::Client, source: &Source) -> Result<Vec<u8>, String> {
    let res = client
        .get(source.url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("download {}", res.status().as_u16()));
    }
    let body = res.bytes().await.map_err(|e| e.to_string())?;
    if hex(&Sha256::digest(&body)) != source.sha256 {
        return Err("checksum mismatch".into());
    }
    Ok(body.to_vec())
}

async fn download(dir: &Path, font: &Font) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut last = String::new();
    for source in &font.sources {
        match fetch(&client, source).await {
            Ok(data) => {
                let part = dir.join(format!("{}.part", font.file));
                tokio::fs::write(&part, data)
                    .await
                    .map_err(|e| e.to_string())?;
                return tokio::fs::rename(&part, dir.join(font.file))
                    .await
                    .map_err(|e| e.to_string());
            }
            Err(e) => {
                eprintln!("[fonts] {}: {e}", source.url);
                last = e;
            }
        }
    }
    Err(last)
}

async fn ensure(dir: &Path, language: &str) {
    let Some(font) = font_for(language) else {
        return;
    };
    if scan(dir).iter().any(|p| covers(p, font.probe)) || system_has(font).await {
        return;
    }
    println!(
        "[fonts] no font for {language} on this system, fetching {}",
        font.file
    );
    match download(dir, font).await {
        Ok(()) => println!("[fonts] {} saved in {}", font.file, dir.display()),
        Err(e) => eprintln!("[fonts] {} not fetched: {e}", font.file),
    }
}

pub async fn follow(core: Arc<Core>, dir: PathBuf) {
    let hub = core.hub.clone();
    let mut state = hub.watch();
    let mut last: Option<String> = None;
    loop {
        let language = state.borrow_and_update().config.language.clone();
        if last.as_ref() != Some(&language) {
            ensure(&dir, &language).await;
            let fonts = scan(&dir);
            hub.update(|s| s.system.fonts = fonts);
            last = Some(language);
        }
        if state.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    #[test]
    fn every_script_has_a_font() {
        for (code, script, _) in livi_i18n::LOCALES {
            assert_eq!(font_for(code).is_some(), script.is_some(), "{code}");
        }
    }

    #[test]
    fn only_font_files_are_offered() {
        let dir = TempDir::new();
        for name in ["b.otf", "a.TTF", "c.ttc", "NotoSansSC.ttf.part", "OFL.txt"] {
            std::fs::write(dir.0.join(name), b"").unwrap();
        }
        let names: Vec<String> = scan(&dir.0)
            .iter()
            .map(|p| {
                Path::new(p)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["a.TTF", "b.otf", "c.ttc"]);
        assert!(scan(&dir.0.join("missing")).is_empty());
    }

    #[test]
    fn coverage_comes_from_the_character_map() {
        let roboto = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../livi-ui/ui/fonts/Roboto-400.ttf"
        );
        assert!(covers(roboto, 'A'));
        assert!(!covers(roboto, '中'));
    }

    #[test]
    fn a_broken_file_covers_nothing() {
        let dir = TempDir::new();
        let path = dir.0.join("broken.ttf");
        std::fs::write(&path, b"not a font").unwrap();
        assert!(!covers(&path.display().to_string(), '中'));
    }

    #[test]
    fn every_source_is_pinned() {
        assert_eq!(hex(&[0x00, 0xab, 0x7f]), "00ab7f");
        for font in &FONTS {
            for source in &font.sources {
                assert_eq!(source.sha256.len(), 64, "{}", source.url);
                assert!(source.url.starts_with("https://"), "{}", source.url);
            }
        }
    }
}
