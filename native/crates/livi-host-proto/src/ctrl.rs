// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Region {
    pub crop_l: f64,
    pub crop_t: f64,
    pub vis_w: f64,
    pub vis_h: f64,
    pub tier_w: f64,
    pub tier_h: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TouchCal {
    pub enabled: bool,
    pub display: String,
    pub matrix: [f64; 6],
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Restart,
    Screen {
        role: String,
        on: bool,
        size: Option<(u32, u32)>,
    },
    Fullscreen {
        role: String,
        on: bool,
    },
    Claim(String),
    Unclaim(String),
    Place {
        tag: String,
        screen: String,
        region: Region,
    },
    Show {
        tag: String,
        shown: bool,
    },
    Aspect {
        role: String,
        size: Option<(u32, u32)>,
    },
    Mode {
        role: String,
        mode: Option<String>,
    },
    Backdrop([u8; 3]),
    Gamma([f64; 5]),
    TouchCals(usize),
    TouchCal {
        name: String,
        cal: TouchCal,
    },
}

fn sized(size: Option<(u32, u32)>) -> Option<(u32, u32)> {
    size.filter(|(w, h)| *w > 0 && *h > 0)
}

fn joined<T: fmt::Display>(f: &mut fmt::Formatter<'_>, values: &[T]) -> fmt::Result {
    for v in values {
        write!(f, " {v}")?;
    }
    Ok(())
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restart => f.write_str("restart"),
            Self::Screen { role, on, size } => {
                write!(f, "screen {role} {}", u8::from(*on))?;
                match sized(*size) {
                    Some((w, h)) => write!(f, " {w} {h}"),
                    None => Ok(()),
                }
            }
            Self::Fullscreen { role, on } => write!(f, "fullscreen {role} {}", u8::from(*on)),
            Self::Claim(tag) => write!(f, "claim {tag}"),
            Self::Unclaim(tag) => write!(f, "unclaim {tag}"),
            Self::Place {
                tag,
                screen,
                region: r,
            } => {
                write!(f, "videocfg {tag} {screen}")?;
                joined(
                    f,
                    &[r.crop_l, r.crop_t, r.vis_w, r.vis_h, r.tier_w, r.tier_h],
                )
            }
            Self::Show { tag, shown } => write!(f, "videoshow {tag} {}", u8::from(*shown)),
            Self::Aspect { role, size } => {
                let (w, h) = sized(*size).unwrap_or((0, 0));
                write!(f, "aspect {role} {w} {h}")
            }
            Self::Mode { role, mode } => {
                let mode = mode.as_deref().filter(|m| !m.is_empty()).unwrap_or("-");
                write!(f, "mode {role} {mode}")
            }
            Self::Backdrop(rgb) => {
                f.write_str("backdrop")?;
                joined(f, rgb)
            }
            Self::Gamma(values) => {
                f.write_str("gamma")?;
                joined(f, values)
            }
            Self::TouchCals(count) => write!(f, "touchcals {count}"),
            Self::TouchCal { name, cal } => {
                let display = if cal.display.is_empty() {
                    "-"
                } else {
                    &cal.display
                };
                write!(f, "touchcal {} {display}", u8::from(cal.enabled))?;
                joined(f, &cal.matrix)?;
                write!(f, " {name}")
            }
        }
    }
}

fn numbers<'a, T: FromStr, const N: usize>(words: impl Iterator<Item = &'a str>) -> Option<[T; N]> {
    let all: Vec<T> = words.map(str::parse).collect::<Result<_, _>>().ok()?;
    all.try_into().ok()
}

fn flag(word: &str) -> Option<bool> {
    match word {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn word<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<String> {
    words.next().map(str::to_string)
}

impl Command {
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut words = line.split(' ').filter(|w| !w.is_empty());
        let cmd = match words.next()? {
            "restart" => Self::Restart,
            "screen" => {
                let role = word(&mut words)?;
                let on = flag(words.next()?)?;
                let rest: Vec<&str> = words.by_ref().collect();
                let size = if rest.is_empty() {
                    None
                } else {
                    let [w, h] = numbers(rest.into_iter())?;
                    sized(Some((w, h)))
                };
                Self::Screen { role, on, size }
            }
            "fullscreen" => Self::Fullscreen {
                role: word(&mut words)?,
                on: flag(words.next()?)?,
            },
            "claim" => Self::Claim(word(&mut words)?),
            "unclaim" => Self::Unclaim(word(&mut words)?),
            "videocfg" => {
                let tag = word(&mut words)?;
                let screen = word(&mut words)?;
                let [crop_l, crop_t, vis_w, vis_h, tier_w, tier_h] = numbers(words.by_ref())?;
                Self::Place {
                    tag,
                    screen,
                    region: Region {
                        crop_l,
                        crop_t,
                        vis_w,
                        vis_h,
                        tier_w,
                        tier_h,
                    },
                }
            }
            "videoshow" => Self::Show {
                tag: word(&mut words)?,
                shown: flag(words.next()?)?,
            },
            "aspect" => {
                let role = word(&mut words)?;
                let [w, h] = numbers(words.by_ref())?;
                Self::Aspect {
                    role,
                    size: sized(Some((w, h))),
                }
            }
            "mode" => {
                let role = word(&mut words)?;
                let mode = words.next()?;
                Self::Mode {
                    role,
                    mode: (mode != "-").then(|| mode.to_string()),
                }
            }
            "backdrop" => Self::Backdrop(numbers(words.by_ref())?),
            "gamma" => Self::Gamma(numbers(words.by_ref())?),
            "touchcals" => Self::TouchCals(words.next()?.parse().ok()?),
            "touchcal" => {
                let mut parts = line.splitn(10, ' ').skip(1);
                let enabled = flag(parts.next()?)?;
                let display = parts.next()?;
                let matrix = numbers(parts.by_ref().take(6))?;
                let name = parts.next().filter(|n| !n.is_empty())?.to_string();
                return Some(Self::TouchCal {
                    name,
                    cal: TouchCal {
                        enabled,
                        display: if display == "-" {
                            String::new()
                        } else {
                            display.to_string()
                        },
                        matrix,
                    },
                });
            }
            _ => return None,
        };
        words.next().is_none().then_some(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(cmd: Command) {
        let line = cmd.to_string();
        assert_eq!(Command::parse(&line), Some(cmd), "{line}");
    }

    #[test]
    fn every_command_reads_back_what_it_wrote() {
        round_trip(Command::Restart);
        round_trip(Command::Screen {
            role: "cluster".into(),
            on: true,
            size: Some((800, 480)),
        });
        round_trip(Command::Screen {
            role: "aux".into(),
            on: false,
            size: None,
        });
        round_trip(Command::Fullscreen {
            role: "main".into(),
            on: true,
        });
        round_trip(Command::Claim("cluster-cluster".into()));
        round_trip(Command::Unclaim("main".into()));
        round_trip(Command::Place {
            tag: "main".into(),
            screen: "main".into(),
            region: Region {
                crop_l: 160.0,
                crop_t: 0.0,
                vis_w: 960.0,
                vis_h: 720.0,
                tier_w: 1280.0,
                tier_h: 720.0,
            },
        });
        round_trip(Command::Show {
            tag: "main".into(),
            shown: false,
        });
        round_trip(Command::Aspect {
            role: "main".into(),
            size: Some((16, 9)),
        });
        round_trip(Command::Aspect {
            role: "main".into(),
            size: None,
        });
        round_trip(Command::Mode {
            role: "cluster".into(),
            mode: Some("1920x720@60".into()),
        });
        round_trip(Command::Mode {
            role: "cluster".into(),
            mode: None,
        });
        round_trip(Command::Backdrop([212, 212, 212]));
        round_trip(Command::Gamma([1.0, 1.2, 1.0, 0.9, 1.0]));
        round_trip(Command::TouchCals(2));
        round_trip(Command::TouchCal {
            name: "ILITEK Multi Touch".into(),
            cal: TouchCal {
                enabled: true,
                display: String::new(),
                matrix: [1.0, 0.0, 0.0, 0.0, -1.0, 1.0],
            },
        });
    }

    #[test]
    fn the_wire_text_stays_as_before() {
        let place = Command::Place {
            tag: "main".into(),
            screen: "main".into(),
            region: Region {
                crop_l: 160.0,
                crop_t: 0.0,
                vis_w: 960.0,
                vis_h: 720.0,
                tier_w: 1280.0,
                tier_h: 720.0,
            },
        };
        assert_eq!(
            place.to_string(),
            "videocfg main main 160 0 960 720 1280 720"
        );
        let cal = Command::TouchCal {
            name: "pad".into(),
            cal: TouchCal {
                enabled: false,
                display: "DSI-1".into(),
                matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.5],
            },
        };
        assert_eq!(cal.to_string(), "touchcal 0 DSI-1 1 0 0 0 1 0.5 pad");
    }

    #[test]
    fn a_touch_calibration_line_names_its_device_last() {
        let Some(Command::TouchCal { name, cal }) =
            Command::parse("touchcal 1 DEL-a0b1-00000001 1 0 0.02 0 0.98 0 ILITEK ILITEK-TP")
        else {
            panic!("a touch calibration");
        };
        assert_eq!(name, "ILITEK ILITEK-TP");
        assert!(cal.enabled);
        assert_eq!(cal.display, "DEL-a0b1-00000001");
        assert_eq!(cal.matrix, [1.0, 0.0, 0.02, 0.0, 0.98, 0.0]);
        let Some(Command::TouchCal { cal: off, .. }) =
            Command::parse("touchcal 0 - 1 0 0 0 1 0 Other")
        else {
            panic!("a touch calibration");
        };
        assert!(!off.enabled && off.display.is_empty());
        assert_eq!(Command::parse("touchcal 1 - 1 0 0 0 1 Short"), None);
        assert_eq!(Command::parse("touchcal 1 - 1 0 0 0 1 0 "), None);
    }

    #[test]
    fn a_broken_line_is_refused_whole() {
        for line in [
            "",
            "claim",
            "screen cluster 2",
            "screen cluster 1 800",
            "videocfg main main 1 2 3",
            "videocfg main main 1 2 3 4 5 x",
            "backdrop 300 0 0",
            "backdrop 1 2 3 4",
            "videoshow main",
            "aspect main 16",
            "touchcal 1 - 1 0 0 0 1",
            "dance main",
        ] {
            assert_eq!(Command::parse(line), None, "{line}");
        }
    }
}
