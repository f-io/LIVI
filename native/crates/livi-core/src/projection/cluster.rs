// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl Projection {
    pub(super) fn cluster_changed(&mut self) {
        if let Some(Link::Cp(id)) = self.active().map(|s| s.link)
            && let Some(layout) = self.phone_layout()
        {
            self.cp.stack(id, StackCmd::ClusterLayout(layout));
        }
        self.keep_cluster();
        self.publish();
    }

    pub(super) fn phone_layout(&self) -> Option<ui_sync::Layout> {
        let drawn = &self.config_seen.car_play_theme;
        if drawn.is_empty() || *drawn != self.cluster.theme.id {
            return None;
        }
        let layout = self.cluster.theme.cluster_layout(&self.cluster.layout)?;
        Some(ui_sync::Layout {
            id: layout.id.clone(),
            palette: self.cluster.palette.clone(),
            active: layout
                .zones
                .iter()
                .map(|z| (z.id.clone(), z.component.clone()))
                .collect(),
        })
    }

    pub(super) fn follow_theme(&mut self, cfg: &Config) {
        let configured = &cfg.car_play_theme;
        let theme = theme_catalog::cached(&self.themes, configured).unwrap_or_else(livi_theme);
        let fetch = theme.id != *configured
            && ThemeId::parse(configured).is_some()
            && self.theme_asked != *configured;
        if theme.id != self.cluster.theme.id {
            self.cluster = kept_cluster(cfg, theme);
        }
        if !fetch {
            return;
        }
        self.theme_asked = configured.clone();
        let (dir, id, ready) = (
            self.themes.clone(),
            configured.clone(),
            self.theme_ready.0.clone(),
        );
        tokio::spawn(async move {
            match theme_catalog::fetch(&dir, &id).await {
                Ok(theme) => {
                    println!("[core] car maker theme {id} fetched");
                    let _ = ready.send(theme);
                }
                Err(e) => eprintln!("[core] car maker theme {id} not fetched: {e}"),
            }
        });
    }

    pub(super) fn theme_fetched(&mut self, theme: Theme) {
        if theme.id == self.config_seen.car_play_theme {
            self.cluster = kept_cluster(&self.config_seen, theme);
            self.cluster_changed();
        }
    }

    pub(super) fn arrange_dashboard(&self, slots: Option<BTreeMap<String, LayoutSlot>>) {
        if let Some(slots) = slots {
            let _ = self
                .settings
                .send(serde_json::json!({ "dashboardLayouts": slots }));
        }
    }

    pub(super) fn keep_cluster(&self) {
        let _ = self.settings.send(serde_json::json!({
            "clusterLayout": self.cluster.layout,
            "clusterPalettes": self.cluster.palettes,
        }));
    }
}
