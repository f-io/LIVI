/// All zero for the whole frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Region {
    pub crop_l: f64,
    pub crop_t: f64,
    pub vis_w: f64,
    pub vis_h: f64,
    pub tier_w: f64,
    pub tier_h: f64,
}

#[derive(Debug, PartialEq)]
pub enum Line {
    /// Core waits for "bound" before the plane of this tag comes.
    Claim(String),
    Place {
        tag: String,
        region: Region,
    },
    Show {
        tag: String,
        shown: bool,
    },
    /// 0 to 255 per channel.
    Backdrop([u8; 3]),
    /// Gamma, contrast and the three gains.
    Gamma([f64; 5]),
}

fn numbers<'a, T: std::str::FromStr, const N: usize>(
    words: impl Iterator<Item = &'a str>,
) -> Option<[T; N]> {
    let all: Vec<T> = words.map(str::parse).collect::<Result<_, _>>().ok()?;
    all.try_into().ok()
}

pub fn parse(line: &str) -> Option<Line> {
    let mut words = line.split_whitespace();
    match words.next()? {
        "claim" => Some(Line::Claim(words.next()?.to_string())),
        "videocfg" => {
            let tag = words.next()?.to_string();
            // Skips the screen, which the plane's tag already gives.
            words.next()?;
            let [crop_l, crop_t, vis_w, vis_h, tier_w, tier_h] = numbers(words)?;
            Some(Line::Place {
                tag,
                region: Region { crop_l, crop_t, vis_w, vis_h, tier_w, tier_h },
            })
        }
        "videoshow" => {
            let tag = words.next()?.to_string();
            let [on] = numbers::<i32, 1>(words)?;
            Some(Line::Show { tag, shown: on != 0 })
        }
        "backdrop" => numbers(words).map(Line::Backdrop),
        "gamma" => numbers(words).map(Line::Gamma),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_core_sends_come_apart() {
        assert_eq!(parse("claim cluster-cluster"), Some(Line::Claim("cluster-cluster".into())));
        assert_eq!(
            parse("videocfg main main 160 0 960 720 1280 720"),
            Some(Line::Place {
                tag: "main".into(),
                region: Region {
                    crop_l: 160.0,
                    crop_t: 0.0,
                    vis_w: 960.0,
                    vis_h: 720.0,
                    tier_w: 1280.0,
                    tier_h: 720.0
                }
            })
        );
        assert_eq!(
            parse("videoshow main 0"),
            Some(Line::Show { tag: "main".into(), shown: false })
        );
        assert_eq!(parse("backdrop 212 212 212"), Some(Line::Backdrop([212, 212, 212])));
        assert_eq!(parse("gamma 1 1.2 1 0.9 1"), Some(Line::Gamma([1.0, 1.2, 1.0, 0.9, 1.0])));
    }

    #[test]
    fn what_a_window_cannot_use_is_left_alone() {
        assert_eq!(parse("screen cluster 1 800 480"), None);
        assert_eq!(parse("unclaim main"), None);
        assert_eq!(parse("restart"), None);
        assert_eq!(parse("videocfg main main 1 2 3"), None);
        assert_eq!(parse("backdrop 300 0 0"), None);
        assert_eq!(parse("claim"), None);
        assert_eq!(parse(""), None);
    }
}
