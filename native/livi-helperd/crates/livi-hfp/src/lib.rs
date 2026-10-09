use std::sync::OnceLock;

pub mod sco;

// Bit 2 CLI, bit 3 voice recognition, bit 4 remote volume. No codec negotiation:
// the AG then defaults to CVSD and SCO carries raw PCM s16le 8kHz.
pub const HF_FEATURES: u32 = (1 << 2) | (1 << 3) | (1 << 4);

const OK: &str = "\r\nOK\r";

fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DEBUG").is_ok_and(|v| v == "1"))
}

#[derive(Default)]
pub struct Slc {
    ag_features: u32,
    sent_bac: bool,
    sent_cind_test: bool,
    sent_cind_read: bool,
    sent_cmer: bool,
    established: bool,
    post: Vec<&'static str>,
    indicators: Vec<String>,
    /// 0 to 5.
    pub battchg: Option<u8>,
    call: u8,
    callsetup: u8,
}

impl Slc {
    pub fn established(&self) -> bool {
        self.established
    }

    /// A call is up, ringing or being dialled.
    pub fn in_call(&self) -> bool {
        self.call > 0 || self.callsetup > 0
    }

    fn indicator(&mut self, index: usize, value: Option<u8>) {
        match self.indicators.get(index).map(String::as_str) {
            Some("battchg") => self.battchg = value,
            Some("call") => self.call = value.unwrap_or(0),
            Some("callsetup") => self.callsetup = value.unwrap_or(0),
            _ => {}
        }
    }

    /// The opening command when no probe has sent it yet.
    pub fn hello() -> String {
        format!("AT+BRSF={HF_FEATURES}\r")
    }

    pub fn on_line(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }
        if debug() {
            println!("[hfp] << {line}");
        }

        if let Some(v) = line.strip_prefix("+BRSF:") {
            self.ag_features = v.trim().parse().unwrap_or(self.ag_features);
            return vec![];
        }
        if line == "OK" {
            if self.established {
                if !self.post.is_empty() {
                    return vec![self.post.remove(0).into()];
                }
                return vec![];
            }
            let both_codec_neg = HF_FEATURES & (1 << 7) != 0 && self.ag_features & (1 << 9) != 0;
            if self.ag_features > 0 && both_codec_neg && !self.sent_bac {
                self.sent_bac = true;
                return vec!["AT+BAC=1,2\r".into()];
            }
            if self.ag_features > 0 && !self.sent_cind_test {
                self.sent_cind_test = true;
                return vec!["AT+CIND=?\r".into()];
            }
            if self.sent_cind_test && !self.sent_cind_read {
                self.sent_cind_read = true;
                return vec!["AT+CIND?\r".into()];
            }
            if self.sent_cind_read && !self.sent_cmer {
                self.sent_cmer = true;
                return vec!["AT+CMER=3,0,0,1\r".into()];
            }
            if self.sent_cmer && !self.established {
                self.established = true;
                println!("[hfp] SLC established");
                // Android drops a silent HF after about 12 s, this dialogue keeps the link alive.
                self.post = vec![
                    "AT+CLIP=1\r",
                    "AT+CCWA=1\r",
                    "AT+CMEE=1\r",
                    "AT+CLCC\r",
                    "AT+VGS=12\r",
                    "AT+VGM=12\r",
                ];
                return vec![self.post.remove(0).into()];
            }
            return vec![];
        }
        if let Some(v) = line.strip_prefix("+CIND:") {
            let v = v.trim();
            if v.starts_with('(') {
                // Test response ("call",(0,1)),... gives the indicator order.
                self.indicators = v.split('"').skip(1).step_by(2).map(str::to_string).collect();
            } else {
                // Read response: current values in the captured order.
                for (i, val) in v.split(',').enumerate() {
                    self.indicator(i, val.trim().parse().ok());
                }
            }
            return vec![];
        }
        if line == "ERROR" {
            return vec![];
        }

        if let Some(v) = line.strip_prefix("AT+BRSF=") {
            self.ag_features = v.trim().parse().unwrap_or(self.ag_features);
            return vec![format!("\r\n+BRSF: {HF_FEATURES}"), OK.into()];
        }
        if line == "AT+CIND=?" {
            return vec![
                "\r\n+CIND: (\"service\",(0,1)),(\"call\",(0,1)),(\"callsetup\",(0-3)),(\"callheld\",(0-2)),(\"signal\",(0-5)),(\"roam\",(0,1)),(\"battchg\",(0-5))".into(),
                OK.into(),
            ];
        }
        if line == "AT+CIND?" {
            return vec!["\r\n+CIND: 1,0,0,0,5,0,5".into(), OK.into()];
        }
        if line.starts_with("AT+CHLD=?") {
            return vec!["\r\n+CHLD: (0,1,2,3)".into(), OK.into()];
        }
        if line.starts_with("AT+BIND=?") {
            return vec!["\r\n+BIND: (1,2)".into(), OK.into()];
        }
        if line.starts_with("AT+BIND?") {
            return vec!["\r\n+BIND: 1,1".into(), "\r\n+BIND: 2,1".into(), OK.into()];
        }
        if let Some(v) = line.strip_prefix("+BCS:") {
            return vec![format!("AT+BCS={}\r", v.trim())];
        }
        if line.starts_with("AT+COPS") {
            if line.contains("=?") {
                return vec![OK.into()];
            }
            if line.contains('?') {
                return vec!["\r\n+COPS: 0,0,\"Carrier\"".into(), OK.into()];
            }
            return vec![OK.into()];
        }
        if let Some(v) = line.strip_prefix("+CIEV:") {
            let mut it = v.trim().split(',');
            let idx: usize = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            let val: u8 = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            // +CIEV indices are 1-based over the +CIND=? order.
            if idx >= 1 {
                self.indicator(idx - 1, Some(val));
            }
            return vec![];
        }
        if line == "RING" || line.starts_with("+CLIP:") {
            return vec![];
        }
        vec![OK.into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(slc: &mut Slc, line: &str) -> Vec<String> {
        slc.on_line(line)
    }

    #[test]
    fn slc_walks_to_established_and_tracks_battchg() {
        let mut slc = Slc::default();
        assert_eq!(Slc::hello(), format!("AT+BRSF={HF_FEATURES}\r"));
        assert!(drive(&mut slc, "+BRSF:4095").is_empty());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CIND=?\r"]);
        assert!(drive(&mut slc, "+CIND: (\"call\",(0,1)),(\"battchg\",(0-5))").is_empty());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CIND?\r"]);
        assert!(drive(&mut slc, "+CIND: 0,4").is_empty());
        assert_eq!(slc.battchg, Some(4));
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CMER=3,0,0,1\r"]);
        assert!(!slc.established());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CLIP=1\r"]);
        assert!(slc.established());
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CCWA=1\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CMEE=1\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+CLCC\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+VGS=12\r"]);
        assert_eq!(drive(&mut slc, "OK"), vec!["AT+VGM=12\r"]);
        assert!(drive(&mut slc, "OK").is_empty());
        assert!(drive(&mut slc, "+CIEV: 2,3").is_empty());
        assert_eq!(slc.battchg, Some(3));
    }

    #[test]
    fn a_call_counts_from_ringing_until_hang_up() {
        let mut slc = Slc::default();
        drive(&mut slc, "+CIND: (\"service\",(0,1)),(\"call\",(0,1)),(\"callsetup\",(0-3))");
        drive(&mut slc, "+CIND: 1,0,0");
        assert!(!slc.in_call());
        drive(&mut slc, "+CIEV: 3,1");
        assert!(slc.in_call());
        drive(&mut slc, "+CIEV: 2,1");
        drive(&mut slc, "+CIEV: 3,0");
        assert!(slc.in_call());
        drive(&mut slc, "+CIEV: 2,0");
        assert!(!slc.in_call());
        drive(&mut slc, "+CIND: 1,1,0");
        assert!(slc.in_call());
    }

    #[test]
    fn answers_ag_initiated_commands() {
        let mut slc = Slc::default();
        let out = drive(&mut slc, "AT+BRSF=511");
        assert_eq!(out, vec![format!("\r\n+BRSF: {HF_FEATURES}"), "\r\nOK\r".to_string()]);
        assert_eq!(drive(&mut slc, "AT+CIND?"), vec!["\r\n+CIND: 1,0,0,0,5,0,5", "\r\nOK\r"]);
        assert_eq!(drive(&mut slc, "+BCS:2"), vec!["AT+BCS=2\r"]);
        assert_eq!(drive(&mut slc, "AT+COPS?"), vec!["\r\n+COPS: 0,0,\"Carrier\"", "\r\nOK\r"]);
        assert_eq!(drive(&mut slc, "AT+WEIRD"), vec!["\r\nOK\r"]);
        assert!(drive(&mut slc, "RING").is_empty());
    }
}
