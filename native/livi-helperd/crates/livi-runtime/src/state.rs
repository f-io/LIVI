use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::livi_sock::SharedTag;

/// Shared helper state: reconnect targets, the tags of the carkit iAP2 sessions, and the
/// phones an iAP2 link is running with. Targets stay in the order LIVI sent them: most
/// recently seen phone first.
#[derive(Default)]
pub struct HelperState {
    reconnect_targets: Mutex<Vec<(String, Option<String>)>>,
    carkit: Mutex<Vec<SharedTag>>,
    links: Mutex<HashSet<String>>,
}

impl HelperState {
    pub fn link_up(&self, mac: &str) {
        self.links.lock().unwrap().insert(mac.to_uppercase());
    }

    pub fn link_down(&self, mac: &str) {
        self.links.lock().unwrap().remove(&mac.to_uppercase());
    }

    /// True while an iAP2 link runs with this phone, from identification to the end.
    pub fn link_active(&self, mac: &str) -> bool {
        self.links.lock().unwrap().contains(&mac.to_uppercase())
    }

    pub fn set_reconnect_targets(&self, targets: Vec<(String, Option<String>)>) {
        let normalized = targets.into_iter().map(|(m, u)| (m.to_uppercase(), u)).collect();
        *self.reconnect_targets.lock().unwrap() = normalized;
    }

    pub fn reconnect_targets(&self) -> Vec<(String, Option<String>)> {
        self.reconnect_targets.lock().unwrap().clone()
    }

    pub fn carkit_started(&self, tag: SharedTag) {
        self.carkit.lock().unwrap().push(tag);
    }

    pub fn carkit_ended(&self, tag: &SharedTag) {
        self.carkit.lock().unwrap().retain(|t| !Arc::ptr_eq(t, tag));
    }

    /// True when this phone (or any phone, when no MAC is given) runs iAP2 over carkit.
    pub fn carkit_blocks(&self, bt_mac: &str) -> bool {
        let sessions = self.carkit.lock().unwrap();
        if bt_mac.is_empty() {
            return !sessions.is_empty();
        }
        sessions.iter().any(|t| {
            t.lock().unwrap().phone_id.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(bt_mac))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventTag;

    fn tag(mac: Option<&str>) -> SharedTag {
        Arc::new(Mutex::new(EventTag { phone_id: mac.map(str::to_string), ..Default::default() }))
    }

    #[test]
    fn blocks_only_the_wired_phone() {
        let state = HelperState::default();
        let wired = tag(Some("0C:6A:C4:4E:F3:2A"));
        state.carkit_started(wired.clone());
        assert!(state.carkit_blocks("0c:6a:c4:4e:f3:2a"));
        assert!(!state.carkit_blocks("AA:BB:CC:DD:EE:FF"));
        state.carkit_ended(&wired);
        assert!(!state.carkit_blocks("0C:6A:C4:4E:F3:2A"));
    }

    #[test]
    fn no_mac_falls_back_to_any_carkit_session() {
        let state = HelperState::default();
        assert!(!state.carkit_blocks(""));
        let unlearned = tag(None);
        state.carkit_started(unlearned.clone());
        assert!(state.carkit_blocks(""));
        // A session that has not learned its phone yet cannot be matched by MAC.
        assert!(!state.carkit_blocks("AA:BB:CC:DD:EE:FF"));
        state.carkit_ended(&unlearned);
        assert!(!state.carkit_blocks(""));
    }

    #[test]
    fn a_phone_with_a_link_is_known_whatever_the_case_of_its_mac() {
        let state = HelperState::default();
        assert!(!state.link_active("0c:6a:c4:4e:f3:2a"));
        state.link_up("0c:6a:c4:4e:f3:2a");
        assert!(state.link_active("0C:6A:C4:4E:F3:2A"));
        state.link_down("0C:6A:C4:4E:F3:2A");
        assert!(!state.link_active("0c:6a:c4:4e:f3:2a"));
    }
}
