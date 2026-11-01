// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::settings::node::{Label, Node, OptionValue, choice};

const MIN_HEIGHT: f64 = 200.0;
const MIN_WIDTH: f64 = 300.0;
const MAX_WIDTH: f64 = 4096.0;
const MAX_HEIGHT: f64 = 2160.0;
const MIN_FPS: f64 = 20.0;
const MAX_FPS: f64 = 60.0;
const MIN_DPI: f64 = 0.0;
const MAX_DPI: f64 = 640.0;
const SAFE_AREA_MIN: f64 = 0.0;
const SAFE_AREA_MAX_WIDTH: f64 = MAX_WIDTH;
const SAFE_AREA_MAX_HEIGHT: f64 = MAX_HEIGHT;
const AREA_STEP: f64 = 2.0;

#[allow(clippy::too_many_arguments)]
fn number(
    key: &'static str,
    text: &'static str,
    icon: &'static str,
    path: &'static str,
    min: f64,
    max: f64,
    step: f64,
    title: &'static str,
) -> Node {
    Node::number(Label::new(key, text), path)
        .icon(icon)
        .range(Some(min), Some(max), Some(step))
        .display_value()
        .page(Label::new(key, title))
}

fn top(path: &'static str, title: &'static str) -> Node {
    number(
        "settings.top",
        "Top",
        "edgeTop",
        path,
        SAFE_AREA_MIN,
        SAFE_AREA_MAX_HEIGHT,
        AREA_STEP,
        title,
    )
}

fn bottom(path: &'static str, title: &'static str) -> Node {
    number(
        "settings.bottom",
        "Bottom",
        "edgeBottom",
        path,
        SAFE_AREA_MIN,
        SAFE_AREA_MAX_HEIGHT,
        AREA_STEP,
        title,
    )
}

fn left(path: &'static str, title: &'static str) -> Node {
    number(
        "settings.left",
        "Left",
        "edgeLeft",
        path,
        SAFE_AREA_MIN,
        SAFE_AREA_MAX_WIDTH,
        AREA_STEP,
        title,
    )
}

fn right(path: &'static str, title: &'static str) -> Node {
    number(
        "settings.right",
        "Right",
        "edgeRight",
        path,
        SAFE_AREA_MIN,
        SAFE_AREA_MAX_WIDTH,
        AREA_STEP,
        title,
    )
}

fn view_area(children: Vec<Node>) -> Node {
    Node::route(
        "viewArea",
        Label::new("settings.viewArea", "View Area"),
        children,
    )
    .icon("viewArea")
}

fn safe_area(children: Vec<Node>) -> Node {
    Node::route(
        "safeArea",
        Label::new("settings.safeArea", "Safe Area"),
        children,
    )
    .icon("safeArea")
}

pub fn schema() -> Node {
    Node::route(
        "video",
        Label::new("settings.video", "Video"),
        vec![
            Node::route(
                "mainVideo",
                Label::new("settings.mainVideo", "Main Video"),
                vec![
                    number(
                        "settings.projectionWidth",
                        "Width",
                        "width",
                        "projectionWidth",
                        MIN_WIDTH,
                        MAX_WIDTH,
                        1.0,
                        "Main Video Width",
                    ),
                    number(
                        "settings.projectionHeight",
                        "Height",
                        "height",
                        "projectionHeight",
                        MIN_HEIGHT,
                        MAX_HEIGHT,
                        1.0,
                        "Main Video Height",
                    ),
                    number(
                        "settings.projectionFps",
                        "FPS",
                        "fps",
                        "projectionFps",
                        MIN_FPS,
                        MAX_FPS,
                        1.0,
                        "Main Video FPS",
                    ),
                    number(
                        "settings.projectionDpi",
                        "DPI",
                        "dpi",
                        "projectionDpi",
                        MIN_DPI,
                        MAX_DPI,
                        1.0,
                        "Main Video DPI",
                    ),
                    view_area(vec![
                        top("projectionViewAreaTop", "Main Screen View Area Top"),
                        bottom("projectionViewAreaBottom", "Main Screen View Area Bottom"),
                        left("projectionViewAreaLeft", "Main Screen View Area Left"),
                        right("projectionViewAreaRight", "Main Screen View Area Right"),
                    ]),
                    safe_area(vec![
                        top("projectionSafeAreaTop", "Main Screen Safe Area Top"),
                        bottom("projectionSafeAreaBottom", "Main Screen Safe Area Bottom"),
                        left("projectionSafeAreaLeft", "Main Screen Safe Area Left"),
                        right("projectionSafeAreaRight", "Main Screen Safe Area Right"),
                        Node::checkbox(
                            Label::new("settings.drawOutside", "Draw Outside"),
                            "projectionSafeAreaDrawOutside",
                        )
                        .icon("drawOutside"),
                    ]),
                ],
            )
            .icon("mainVideo"),
            Node::route(
                "clusterVideo",
                Label::new("settings.clusterVideo", "Cluster Video"),
                vec![
                    number(
                        "settings.clusterWidth",
                        "Width",
                        "width",
                        "clusterWidth",
                        MIN_WIDTH,
                        MAX_WIDTH,
                        1.0,
                        "Cluster Video Width",
                    ),
                    number(
                        "settings.clusterHeight",
                        "Height",
                        "height",
                        "clusterHeight",
                        MIN_HEIGHT,
                        MAX_HEIGHT,
                        1.0,
                        "Cluster Video Height",
                    ),
                    number(
                        "settings.clusterFps",
                        "FPS",
                        "fps",
                        "clusterFps",
                        MIN_FPS,
                        MAX_FPS,
                        1.0,
                        "Cluster Video FPS",
                    ),
                    number(
                        "settings.clusterDpi",
                        "DPI",
                        "dpi",
                        "clusterDpi",
                        MIN_DPI,
                        MAX_DPI,
                        1.0,
                        "Cluster Video DPI",
                    ),
                    view_area(vec![
                        top("clusterViewAreaTop", "Cluster Screen View Area Top"),
                        bottom("clusterViewAreaBottom", "Cluster Screen View Area Bottom"),
                        left("clusterViewAreaLeft", "Cluster Screen View Area Left"),
                        right("clusterViewAreaRight", "Cluster Screen View Area Right"),
                    ]),
                    safe_area(vec![
                        top("clusterSafeAreaTop", "Cluster Screen Safe Area Top"),
                        bottom("clusterSafeAreaBottom", "Cluster Screen Safe Area Bottom"),
                        left("clusterSafeAreaLeft", "Cluster Screen Safe Area Left"),
                        right("clusterSafeAreaRight", "Cluster Screen Safe Area Right"),
                    ]),
                    Node::select(
                        Label::new("settings.clusterContent", "CarPlay Content"),
                        "carPlayClusterContent",
                        vec![
                            choice(
                                Label::new("settings.clusterContentMap", "Map"),
                                OptionValue::Text("map"),
                            ),
                            choice(
                                Label::new("settings.clusterContentManeuver", "Maneuver"),
                                OptionValue::Text("maneuver"),
                            ),
                            choice(
                                Label::new("settings.clusterContentAuto", "Automatic"),
                                OptionValue::Text("auto"),
                            ),
                        ],
                    )
                    .icon("clusterContent")
                    .display_value()
                    .page(Label::new("settings.clusterContent", "CarPlay Content")),
                    Node::checkbox(
                        Label::new("settings.clusterCompass", "Compass"),
                        "carPlayClusterCompass",
                    )
                    .icon("compass"),
                    Node::checkbox(
                        Label::new("settings.clusterEta", "Arrival Time"),
                        "carPlayClusterEta",
                    )
                    .icon("eta"),
                    Node::checkbox(
                        Label::new("settings.clusterSpeedLimit", "Speed Limit"),
                        "carPlayClusterSpeedLimit",
                    )
                    .icon("speedLimit"),
                ],
            )
            .icon("clusterVideo"),
        ],
    )
    .icon("video")
}
