// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label {
    pub key: Option<&'static str>,
    pub text: &'static str,
    pub unreadable: Option<([u8; 4], &'static str)>,
}

impl Label {
    pub const fn new(key: &'static str, text: &'static str) -> Self {
        Self {
            key: Some(key),
            text,
            unreadable: None,
        }
    }

    pub const fn plain(text: &'static str) -> Self {
        Self {
            key: None,
            text,
            unreadable: None,
        }
    }

    pub fn native(text: &'static str, script: Option<[u8; 4]>, latin: &'static str) -> Self {
        Self {
            key: None,
            text,
            unreadable: script.map(|s| (s, latin)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Plain,
    Percent,
    Suffix(&'static str),
    DashIfNegative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    NotLinux,
    DisplaysSettable,
    DisplaysNotSettable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    Cluster,
    Aux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radio {
    Wifi,
    Bt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionSource {
    AudioSinks,
    AudioSources,
    Connectors,
    DisplayModes(Screen),
    WifiBands,
    WifiChannels,
    WifiCountries,
    WifiInterfaces,
    BtAdapters,
    DabGains,
    Cameras,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnPick {
    SwitchDongleOn(Radio),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitSource {
    Speed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptionValue {
    Text(&'static str),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub label: Label,
    pub value: OptionValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomPage {
    About,
    CarReadings,
    ColorCalibration,
    ContrastGammaCalibration,
    DabReceiver,
    DabStations,
    DashboardLayouts,
    GpsHwInfo,
    GpsInfo,
    IconUploader,
    MfiCertificate,
    PowerOff,
    Restart,
    SoftwareUpdate,
    Touch(Screen),
    WifiLinkInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Route {
        route: &'static str,
        children: Vec<Node>,
    },
    Checkbox,
    Toggle,
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
        default: Option<f64>,
        unit: Option<UnitSource>,
    },
    Text {
        min_len: Option<usize>,
        max_len: Option<usize>,
        url: bool,
    },
    Color,
    Select {
        options: Vec<Choice>,
        source: Option<OptionSource>,
        label_path: Option<&'static str>,
        read_only: Option<Condition>,
        on_pick: Option<OnPick>,
    },
    Slider,
    KeyBinding {
        binding: &'static str,
    },
    Custom(CustomPage),
    BtDeviceList,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub label: Label,
    pub path: &'static str,
    pub icon: Option<&'static str>,
    pub page_title: Option<Label>,
    pub display_value: bool,
    pub shown: Shown,
    pub disabled: Option<Condition>,
}

impl Node {
    pub fn new(kind: Kind, label: Label, path: &'static str) -> Self {
        Self {
            kind,
            label,
            path,
            icon: None,
            page_title: None,
            display_value: false,
            shown: Shown::Plain,
            disabled: None,
        }
    }

    pub fn route(route: &'static str, label: Label, children: Vec<Node>) -> Self {
        Self::new(Kind::Route { route, children }, label, "")
    }

    pub fn checkbox(label: Label, path: &'static str) -> Self {
        Self::new(Kind::Checkbox, label, path)
    }

    pub fn toggle(label: Label, path: &'static str) -> Self {
        Self::new(Kind::Toggle, label, path)
    }

    pub fn slider(label: Label, path: &'static str) -> Self {
        Self::new(Kind::Slider, label, path)
    }

    pub fn color(label: Label, path: &'static str) -> Self {
        Self::new(Kind::Color, label, path)
    }

    pub fn number(label: Label, path: &'static str) -> Self {
        Self::new(
            Kind::Number {
                min: None,
                max: None,
                step: None,
                default: None,
                unit: None,
            },
            label,
            path,
        )
    }

    pub fn text(label: Label, path: &'static str) -> Self {
        Self::new(
            Kind::Text {
                min_len: None,
                max_len: None,
                url: false,
            },
            label,
            path,
        )
    }

    pub fn select(label: Label, path: &'static str, options: Vec<Choice>) -> Self {
        Self::new(
            Kind::Select {
                options,
                source: None,
                label_path: None,
                read_only: None,
                on_pick: None,
            },
            label,
            path,
        )
    }

    pub fn key_binding(label: Label, path: &'static str, binding: &'static str) -> Self {
        Self::new(Kind::KeyBinding { binding }, label, path)
    }

    pub fn custom(label: Label, path: &'static str, page: CustomPage) -> Self {
        Self::new(Kind::Custom(page), label, path)
    }

    pub fn bt_device_list(label: Label, path: &'static str) -> Self {
        Self::new(Kind::BtDeviceList, label, path)
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn page(mut self, title: Label) -> Self {
        self.page_title = Some(title);
        self
    }

    pub fn display_value(mut self) -> Self {
        self.display_value = true;
        self
    }

    pub fn shown(mut self, shown: Shown) -> Self {
        self.shown = shown;
        self
    }

    pub fn disabled(mut self, when: Condition) -> Self {
        self.disabled = Some(when);
        self
    }

    pub fn range(mut self, lo: Option<f64>, hi: Option<f64>, by: Option<f64>) -> Self {
        if let Kind::Number { min, max, step, .. } = &mut self.kind {
            *min = lo;
            *max = hi;
            *step = by;
        }
        self
    }

    pub fn default_number(mut self, value: f64) -> Self {
        if let Kind::Number { default, .. } = &mut self.kind {
            *default = Some(value);
        }
        self
    }

    pub fn unit(mut self, source: UnitSource) -> Self {
        if let Kind::Number { unit, .. } = &mut self.kind {
            *unit = Some(source);
        }
        self
    }

    pub fn length(mut self, lo: Option<usize>, hi: Option<usize>) -> Self {
        if let Kind::Text {
            min_len, max_len, ..
        } = &mut self.kind
        {
            *min_len = lo;
            *max_len = hi;
        }
        self
    }

    pub fn url(mut self) -> Self {
        if let Kind::Text { url, .. } = &mut self.kind {
            *url = true;
        }
        self
    }

    pub fn options_from(mut self, from: OptionSource) -> Self {
        if let Kind::Select { source, .. } = &mut self.kind {
            *source = Some(from);
        }
        self
    }

    pub fn label_path(mut self, path: &'static str) -> Self {
        if let Kind::Select { label_path, .. } = &mut self.kind {
            *label_path = Some(path);
        }
        self
    }

    pub fn read_only(mut self, when: Condition) -> Self {
        if let Kind::Select { read_only, .. } = &mut self.kind {
            *read_only = Some(when);
        }
        self
    }

    pub fn on_pick(mut self, action: OnPick) -> Self {
        if let Kind::Select { on_pick, .. } = &mut self.kind {
            *on_pick = Some(action);
        }
        self
    }

    pub fn children(&self) -> &[Node] {
        match &self.kind {
            Kind::Route { children, .. } => children,
            _ => &[],
        }
    }

    pub fn route_key(&self) -> Option<&'static str> {
        match &self.kind {
            Kind::Route { route, .. } => Some(route),
            _ => None,
        }
    }
}

pub fn choice(label: Label, value: OptionValue) -> Choice {
    Choice { label, value }
}

pub fn text_choice(text: &'static str, value: &'static str) -> Choice {
    choice(Label::plain(text), OptionValue::Text(value))
}

pub fn number_choice(text: &'static str, value: f64) -> Choice {
    choice(Label::plain(text), OptionValue::Number(value))
}
