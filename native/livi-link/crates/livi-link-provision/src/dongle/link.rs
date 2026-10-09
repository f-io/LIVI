use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Status {
    pub model: String,
    pub target: String,
    pub version: String,
    pub build: String,
}

pub fn status(host: &str) -> Option<Status> {
    ureq::get(&format!("http://{host}/api/status"))
        .config()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()
}

pub fn bundle(target: &str) -> Option<&'static [u8]> {
    let bytes = match target {
        "v821b_aic8800d80" => super::riscv::v821b::V821B_LFWB,
        "ax520_aic8800d80" => super::arm::ax520::AX520_LFWB,
        "imx6ul_iw416" => super::arm::imx6ul::IMX6UL_LFWB,
        "imx6ul_rtl8822cs" => super::arm::imx6ul::IMX6UL_RTL8822CS_LFWB,
        "imx6ul_rtl8822bs" => super::arm::imx6ul::IMX6UL_RTL8822BS_LFWB,
        _ => return None,
    };
    (!bytes.is_empty()).then_some(bytes)
}

/// The dongle writes and verifies every changed image before it answers, hence the long timeout.
pub fn update(host: &str, bundle: &[u8]) -> Result<String, String> {
    let mut reply = ureq::post(&format!("http://{host}/api/flash"))
        .header("Content-Type", "application/octet-stream")
        .config()
        .timeout_global(Some(Duration::from_secs(900)))
        // A refusal still carries the dongle's reason in its body.
        .http_status_as_error(false)
        .build()
        .send(bundle)
        .map_err(|e| format!("upload: {e}"))?;
    let body = reply.body_mut().read_to_string().map_err(|e| format!("reply: {e}"))?;
    answer(&body)
}

pub fn refused(error: &str) -> Option<u8> {
    error.strip_prefix("image type ")?.split_once(" is not for this dongle")?.0.parse().ok()
}

/// The rest of the bundle brings the firmware that accepts the dropped type.
pub fn without(bundle: &[u8], typ: u8) -> Result<Vec<u8>, String> {
    let images = super::lfwb::unpack(bundle)?;
    let rest: Vec<(u8, &[u8])> =
        images.iter().filter(|(t, _)| *t != typ).map(|(t, data)| (*t, data.as_slice())).collect();
    if rest.is_empty() {
        return Err(format!("the bundle holds nothing but image type {typ}"));
    }
    Ok(super::lfwb::pack(&rest))
}

fn answer(body: &str) -> Result<String, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|_| format!("the dongle answered {body:?}"))?;
    let text = |key: &str| json.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    if json.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        Ok(text("message"))
    } else {
        Err(text("error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_api_answer_is_a_message_or_an_error() {
        assert_eq!(
            answer(r#"{"ok":true,"message":"mtdblock3 written, rebooting"}"#).unwrap(),
            "mtdblock3 written, rebooting"
        );
        assert_eq!(
            answer(r#"{"ok":false,"error":"bundle magic mismatch"}"#).unwrap_err(),
            "bundle magic mismatch"
        );
        assert!(answer("not found").is_err());
    }

    #[test]
    fn a_refused_image_type_is_read_from_the_error() {
        assert_eq!(refused("image type 2 is not for this dongle, nothing written"), Some(2));
        assert_eq!(refused("image type 12 is not for this dongle"), Some(12));
        assert_eq!(refused("bundle magic mismatch"), None);
    }

    #[test]
    fn the_rest_of_a_bundle_goes_without_the_refused_type() {
        let bundle =
            crate::dongle::lfwb::pack(&[(2, b"kernel".as_slice()), (3, b"hsqs-rootfs".as_slice())]);
        let rest = crate::dongle::lfwb::unpack(&without(&bundle, 2).unwrap()).unwrap();
        assert_eq!(rest, vec![(3, b"hsqs-rootfs".to_vec())]);
        let only = crate::dongle::lfwb::pack(&[(2, b"kernel".as_slice())]);
        assert!(without(&only, 2).is_err());
    }

    #[test]
    fn only_known_targets_have_a_bundle() {
        assert!(bundle("some_other_board").is_none());
        assert!(bundle("").is_none());
    }
}
