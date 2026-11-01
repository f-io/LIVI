// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::Read;
use std::path::{Path, PathBuf};

use livi_core_proto::theme::{DisplayKind, Palette, Theme, ThemeDisplay, ThemeLayout, Zone};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CATALOG: &str = "https://gdmf.apple.com/v2/assets";
const ROOT_URL: &str = "https://www.apple.com/appleca/AppleIncRootCertificate.cer";
const ROOT_SHA256: &str = "b0b1730ecbc7ff4505142c49f1295e6eda6bcaed7e2c68c5be91b5a11001f024";
const PACKAGE: &str = "AssetData/CarPlayPhone.assets/package.json";
const STYLES: &str = "AssetData/CarPlayPhone.assets/theme.json";
const MAP_COMPONENT: &str = "DCA_Map";
const TAIL: u64 = 65_557;

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeId {
    pub asset: String,
    pub version: u32,
}

impl ThemeId {
    pub fn parse(configured: &str) -> Option<Self> {
        let mut parts = configured.split(':');
        let asset = parts.next().filter(|a| !a.is_empty())?.to_string();
        let version = parts.next()?.parse().ok()?;
        Some(Self { asset, version })
    }

    fn file(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}-{}.json", self.asset, self.version))
    }
}

pub fn cached(dir: &Path, configured: &str) -> Option<Theme> {
    let text = std::fs::read_to_string(ThemeId::parse(configured)?.file(dir)).ok()?;
    serde_json::from_str::<Theme>(&text)
        .ok()
        .filter(|t| t.id == configured)
}

pub async fn fetch(dir: &Path, configured: &str) -> Result<Theme, String> {
    let id = ThemeId::parse(configured).ok_or_else(|| format!("not a theme id: {configured}"))?;
    let catalog = catalog_client().await?;
    let url = package_url(&catalog, &id).await?;
    let plain = reqwest::Client::new();
    let index = ZipIndex::read(&plain, &url).await?;
    let package = json_entry(&plain, &url, &index, PACKAGE).await?;
    let styles = json_entry(&plain, &url, &index, STYLES).await?;
    let theme = from_package(configured, &package, &styles)
        .ok_or_else(|| format!("{configured}: the package has no cluster"))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string(&theme).map_err(|e| e.to_string())?;
    std::fs::write(id.file(dir), text).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(theme)
}

async fn catalog_client() -> Result<reqwest::Client, String> {
    let der = reqwest::get(ROOT_URL).await.map_err(|e| e.to_string())?;
    let der = der
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await;
    let der = der.map_err(|e| e.to_string())?;
    if hex(&Sha256::digest(&der)) != ROOT_SHA256 {
        return Err("Apple's root certificate does not match".into());
    }
    let root = reqwest::Certificate::from_der(&der).map_err(|e| e.to_string())?;
    reqwest::Client::builder()
        .tls_certs_only([root])
        .build()
        .map_err(|e| e.to_string())
}

async fn package_url(client: &reqwest::Client, id: &ThemeId) -> Result<String, String> {
    let ask = json!({
        "AssetAudience": "0c88076f-c292-4dad-95e7-304db9d29d34",
        "AssetType": "com.apple.MobileAsset.CarDDPAssets",
        "ClientVersion": 2,
        "ProductType": "iPhone17,1",
        "BuildVersion": "24A446",
        "ProductVersion": "27.0",
        "HWModelStr": "D93AP",
        "DeviceName": "iPhone",
        "ClientData": { "AllowXmlFallback": false, "DeviceAccessClient": "softwareupdated" },
    });
    let ask = client
        .post(CATALOG)
        .header("Content-Type", "application/json")
        .body(ask.to_string());
    let answer = ask.send().await.map_err(|e| e.to_string())?;
    let jwt = answer
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await;
    let assets = catalog_assets(&jwt.map_err(|e| e.to_string())?)
        .ok_or("the theme catalog answered without assets")?;
    asset_url(&assets, id)
        .ok_or_else(|| format!("{}:{} is not in the catalog", id.asset, id.version))
}

fn catalog_assets(jwt: &str) -> Option<Vec<Value>> {
    use base64::Engine;
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let body: Value = serde_json::from_slice(&bytes).ok()?;
    body.get("Assets")?.as_array().cloned()
}

fn asset_url(assets: &[Value], id: &ThemeId) -> Option<String> {
    let found = assets.iter().find(|a| {
        a.get("ContentIdentifier").and_then(Value::as_str) == Some(&id.asset)
            && a.get("_ContentVersion").and_then(Value::as_u64) == Some(u64::from(id.version))
    })?;
    let base = found.get("__BaseURL")?.as_str()?;
    let path = found.get("__RelativePath")?.as_str()?;
    Some(format!("{base}{path}"))
}

#[derive(Debug, Clone, PartialEq)]
struct ZipEntry {
    name: String,
    deflated: bool,
    size: u64,
    offset: u64,
}

struct ZipIndex {
    entries: Vec<ZipEntry>,
}

impl ZipIndex {
    async fn read(client: &reqwest::Client, url: &str) -> Result<Self, String> {
        let tail = client
            .get(url)
            .header("Range", format!("bytes=-{TAIL}"))
            .send()
            .await;
        let tail = tail
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let total = tail
            .headers()
            .get("Content-Range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or("the package server ignores byte ranges")?;
        let tail = tail.bytes().await.map_err(|e| e.to_string())?;
        let (offset, size) = directory_at(&tail).ok_or("the package is no zip")?;
        let start = total - tail.len() as u64;
        let directory = if offset >= start {
            let from = (offset - start) as usize;
            tail.get(from..from + size as usize)
                .ok_or("the zip directory is cut")?
                .to_vec()
        } else {
            range(client, url, offset, size).await?
        };
        Ok(Self {
            entries: entries(&directory),
        })
    }
}

async fn range(
    client: &reqwest::Client,
    url: &str,
    from: u64,
    len: u64,
) -> Result<Vec<u8>, String> {
    let bytes = format!("bytes={from}-{}", from + len.max(1) - 1);
    let answer = client
        .get(url)
        .header("Range", bytes)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let answer = answer.error_for_status().map_err(|e| e.to_string())?;
    Ok(answer.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

async fn json_entry(
    client: &reqwest::Client,
    url: &str,
    index: &ZipIndex,
    name: &str,
) -> Result<Value, String> {
    let entry = index
        .entries
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| format!("no {name}"))?;
    let head = range(client, url, entry.offset, 30).await?;
    let skip = local_data_start(&head).ok_or_else(|| format!("{name}: bad local header"))?;
    let data = range(client, url, entry.offset + skip, entry.size).await?;
    let bytes = if entry.deflated {
        inflate(&data)?
    } else {
        data
    };
    serde_json::from_slice(&bytes).map_err(|e| format!("{name}: {e}"))
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn directory_at(tail: &[u8]) -> Option<(u64, u64)> {
    let at = tail
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])?;
    Some((
        u64::from(u32_at(tail, at + 16)?),
        u64::from(u32_at(tail, at + 12)?),
    ))
}

fn entries(directory: &[u8]) -> Vec<ZipEntry> {
    let mut out = Vec::new();
    let mut at = 0;
    while u32_at(directory, at) == Some(0x0201_4b50) {
        let field = |o| u16_at(directory, at + o).map(usize::from);
        let (Some(method), Some(name_len), Some(extra), Some(comment)) =
            (field(10), field(28), field(30), field(32))
        else {
            break;
        };
        let (Some(size), Some(offset)) = (u32_at(directory, at + 20), u32_at(directory, at + 42))
        else {
            break;
        };
        let Some(name) = directory.get(at + 46..at + 46 + name_len) else {
            break;
        };
        out.push(ZipEntry {
            name: String::from_utf8_lossy(name).into_owned(),
            deflated: method == 8,
            size: u64::from(size),
            offset: u64::from(offset),
        });
        at += 46 + name_len + extra + comment;
    }
    out
}

fn local_data_start(head: &[u8]) -> Option<u64> {
    if u32_at(head, 0)? != 0x0403_4b50 {
        return None;
    }
    Some(30 + u64::from(u16_at(head, 26)?) + u64::from(u16_at(head, 28)?))
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_package(id: &str, package: &Value, styles: &Value) -> Option<Theme> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let displays: Vec<ThemeDisplay> = package
        .get("displays")?
        .as_array()?
        .iter()
        .filter_map(|d| {
            let kind = match d.get("type")?.as_str()? {
                "cluster" => DisplayKind::Cluster,
                "centerConsole" => DisplayKind::Center,
                _ => return None,
            };
            let layouts = match kind {
                DisplayKind::Cluster => d
                    .get("layouts")?
                    .as_array()?
                    .iter()
                    .filter_map(|l| layout(l, styles))
                    .collect(),
                DisplayKind::Center => Vec::new(),
            };
            Some(ThemeDisplay {
                id: text(d, "id")?,
                kind,
                layouts,
            })
        })
        .collect();
    displays.iter().find(|d| d.kind == DisplayKind::Cluster)?;
    let palettes = styles
        .get("palettes")
        .and_then(Value::as_object)
        .map(|all| {
            all.iter()
                .filter_map(|(id, p)| {
                    let accent = p.get("accentColor")?;
                    Some(Palette {
                        id: id.clone(),
                        accent_dark: text(accent, "darkMode")?,
                        accent_light: text(accent, "lightMode")?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Theme {
        id: id.to_string(),
        displays,
        palettes,
    })
}

fn layout(l: &Value, styles: &Value) -> Option<ThemeLayout> {
    let style = l.get("layoutStyleID")?.as_str()?;
    let default = styles
        .get("defaultPaletteIDs")
        .and_then(|d| d.get(style))
        .and_then(Value::as_str);
    let offered = styles
        .get("layoutStyles")
        .and_then(|s| s.get(style))
        .and_then(Value::as_object);
    let mut palettes: Vec<String> = default.map(str::to_string).into_iter().collect();
    for id in offered.into_iter().flat_map(|o| o.keys()) {
        if Some(id.as_str()) != default {
            palettes.push(id.clone());
        }
    }
    let zones =
        l.get("zones")
            .and_then(Value::as_array)
            .map(|zones| {
                zones
                    .iter()
                    .filter_map(|z| {
                        let map = z.get("components")?.as_array()?.iter().find(|c| {
                            c.get("name").and_then(Value::as_str) == Some(MAP_COMPONENT)
                        })?;
                        Some(Zone {
                            id: z.get("id")?.as_str()?.to_string(),
                            component: map.get("id")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
    Some(ThemeLayout {
        id: l.get("id")?.as_str()?.to_string(),
        name: l.get("displayName")?.as_str()?.to_string(),
        palettes,
        zones,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> Value {
        json!({ "displays": [
            { "id": "Cluster_Display", "type": "cluster", "layouts": [
                { "id": "L1", "displayName": "Rings", "layoutStyleID": "RingStyle", "zones": [
                    { "id": "Z1", "components": [
                        { "id": "C-adas", "name": "DCA_ADAS" },
                        { "id": "C-map", "name": "DCA_Map" }
                    ] },
                    { "id": "Z2", "components": [{ "id": "C-speed", "name": "Speedometer" }] }
                ] },
                { "id": "L2", "displayName": "Full map", "layoutStyleID": "MapStyle", "zones": [] },
                { "displayName": "no id" }
            ] },
            { "id": "Center_Display", "type": "centerConsole", "layouts": [{ "id": "C1" }] },
            { "id": "Head_Up", "type": "headUp" }
        ] })
    }

    fn styles() -> Value {
        json!({
            "defaultPaletteIDs": { "RingStyle": "green", "MapStyle": "vibrant" },
            "layoutStyles": {
                "RingStyle": { "blue": {}, "green": {}, "red": {} },
                "MapStyle": { "vibrant": {} }
            },
            "palettes": {
                "green": { "accentColor": { "darkMode": "#00ff00ff", "lightMode": "#008800ff" } },
                "broken": {}
            }
        })
    }

    #[test]
    fn the_package_becomes_the_theme_the_cluster_runs() {
        let theme = from_package("ABC:7:19.0", &package(), &styles()).unwrap();
        assert_eq!(theme.id, "ABC:7:19.0");
        let ids: Vec<_> = theme
            .displays
            .iter()
            .map(|d| (d.id.as_str(), d.kind))
            .collect();
        assert_eq!(
            ids,
            [
                ("Cluster_Display", DisplayKind::Cluster),
                ("Center_Display", DisplayKind::Center)
            ]
        );
        let layouts = &theme.cluster().unwrap().layouts;
        assert_eq!(layouts.len(), 2, "a layout without an id is left out");
        assert_eq!(
            (layouts[0].id.as_str(), layouts[0].name.as_str()),
            ("L1", "Rings")
        );
        assert_eq!(
            layouts[0].palettes,
            ["green", "blue", "red"],
            "the default first"
        );
        assert_eq!(
            layouts[0].zones,
            [Zone {
                id: "Z1".into(),
                component: "C-map".into()
            }],
            "only zones that can show the map, with the map"
        );
        assert_eq!(layouts[1].palettes, ["vibrant"]);
        assert_eq!(
            theme.palettes,
            [Palette {
                id: "green".into(),
                accent_dark: "#00ff00ff".into(),
                accent_light: "#008800ff".into()
            }]
        );
        assert!(
            theme.displays[1].layouts.is_empty(),
            "the phone draws the centre"
        );
    }

    #[test]
    fn a_package_without_a_cluster_is_no_theme() {
        let package = json!({ "displays": [{ "id": "C", "type": "centerConsole" }] });
        assert_eq!(from_package("A:1", &package, &styles()), None);
        assert_eq!(from_package("A:1", &json!({}), &styles()), None);
        let theme = from_package("A:1", &self::package(), &json!({})).unwrap();
        assert!(theme.palettes.is_empty());
        assert!(theme.cluster().unwrap().layouts[0].palettes.is_empty());
    }

    #[test]
    fn the_id_names_asset_and_version() {
        let id = ThemeId::parse("44C9FA31EC:985:19.0").unwrap();
        assert_eq!(
            id,
            ThemeId {
                asset: "44C9FA31EC".into(),
                version: 985
            }
        );
        assert_eq!(ThemeId::parse("livi"), None);
        assert_eq!(ThemeId::parse(":1"), None);
        assert_eq!(ThemeId::parse("A:x"), None);
    }

    #[test]
    fn the_catalog_token_lists_the_package() {
        use base64::Engine;
        let assets = json!({ "Assets": [
            { "ContentIdentifier": "A", "_ContentVersion": 1, "__BaseURL": "https://x/", "__RelativePath": "old.zip" },
            { "ContentIdentifier": "A", "_ContentVersion": 2, "__BaseURL": "https://x/", "__RelativePath": "a.zip" }
        ] });
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(assets.to_string());
        let found = catalog_assets(&format!("head.{payload}.sig")).unwrap();
        let id = ThemeId {
            asset: "A".into(),
            version: 2,
        };
        assert_eq!(asset_url(&found, &id).as_deref(), Some("https://x/a.zip"));
        assert_eq!(
            asset_url(
                &found,
                &ThemeId {
                    asset: "B".into(),
                    version: 2
                }
            ),
            None
        );
        assert_eq!(catalog_assets("no token"), None);
    }

    fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        use std::io::Write;
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, data, deflate) in files {
            let body = if *deflate {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), Default::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let offset = out.len() as u32;
            let method: u16 = if *deflate { 8 } else { 0 };
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend([0; 4]);
            out.extend(method.to_le_bytes());
            out.extend([0; 12]);
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(3u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend([1, 2, 3]);
            out.extend(&body);
            directory.extend(0x0201_4b50u32.to_le_bytes());
            directory.extend([0; 6]);
            directory.extend(method.to_le_bytes());
            directory.extend([0; 8]);
            directory.extend((body.len() as u32).to_le_bytes());
            directory.extend((data.len() as u32).to_le_bytes());
            directory.extend((name.len() as u16).to_le_bytes());
            directory.extend([0; 12]);
            directory.extend(offset.to_le_bytes());
            directory.extend(name.as_bytes());
        }
        let at = out.len() as u32;
        out.extend(&directory);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0; 8]);
        out.extend((directory.len() as u32).to_le_bytes());
        out.extend(at.to_le_bytes());
        out.extend([0; 2]);
        out
    }

    #[test]
    fn the_zip_directory_points_at_each_file() {
        let archive = zip(&[
            ("a.json", b"{\"a\":1}", false),
            (PACKAGE, b"{\"b\":22}", true),
        ]);
        let (offset, size) = directory_at(&archive).unwrap();
        let found = entries(&archive[offset as usize..(offset + size) as usize]);
        assert_eq!(found.len(), 2);
        assert!(!found[0].deflated && found[1].deflated);
        for (entry, want) in found.iter().zip([json!({ "a": 1 }), json!({ "b": 22 })]) {
            let at = entry.offset as usize;
            let skip = local_data_start(&archive[at..]).unwrap() as usize;
            let data = &archive[at + skip..at + skip + entry.size as usize];
            let bytes = if entry.deflated {
                inflate(data).unwrap()
            } else {
                data.to_vec()
            };
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), want);
        }
        assert_eq!(directory_at(b"nothing"), None);
        assert_eq!(local_data_start(b"PK\x01\x02"), None);
        assert!(entries(b"PK\x01\x02short").is_empty());
        assert!(inflate(&[0xff, 0xff]).is_err());
    }

    #[test]
    fn a_fetched_theme_is_kept_per_version() {
        let dir = std::env::temp_dir().join(format!("livi-themes-{}", std::process::id()));
        let theme = from_package("ABC:7:19.0", &package(), &styles()).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let id = ThemeId::parse(&theme.id).unwrap();
        std::fs::write(id.file(&dir), serde_json::to_string(&theme).unwrap()).unwrap();
        assert_eq!(cached(&dir, "ABC:7:19.0"), Some(theme));
        assert_eq!(
            cached(&dir, "ABC:7:20.0"),
            None,
            "another SDK is another theme id"
        );
        assert_eq!(cached(&dir, "ABC:8:19.0"), None);
        assert_eq!(cached(&dir, "livi"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
