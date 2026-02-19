pub(crate) const FEATURES: &[(&str, &str, &str)] = &[
    ("tinyvg", "TinyVG", "Vector graphics support"),
    (
        "built-in-shaders",
        "Built-in shaders",
        "Pre-made visual effects (glow, holographic, gradient, etc.)",
    ),
    (
        "shader-pipeline",
        "Shader pipeline",
        "Custom shader compilation with SPIR-V Cross (adds build.rs)",
    ),
    (
        "text-styling",
        "Text styling",
        "Rich text with inline formatting",
    ),
];

pub(crate) const BUILD_RS: &str = r#"fn main() {
    ply_engine::shader_build::ShaderBuild::new()
        .build();
}
"#;

pub(crate) fn generate_cargo_toml(name: &str, features: &[&str]) -> String {
    let mut toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
"#
    );

    let mut ply_features: Vec<&str> = Vec::new();
    for &key in features {
        if key != "shader-pipeline" {
            ply_features.push(key);
        }
    }

    if ply_features.is_empty() {
        toml.push_str(
            "ply-engine = { git = \"https://github.com/TheRedDeveloper/ply-engine\" }\n",
        );
    } else {
        let feat_str = ply_features
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        toml.push_str(&format!(
            "ply-engine = {{ git = \"https://github.com/TheRedDeveloper/ply-engine\", features = [{feat_str}] }}\n"
        ));
    }

    toml.push_str(
        "macroquad = { version = \"0.4\", git = \"https://github.com/TheRedDeveloper/macroquad-fix\" }\n",
    );

    if features.contains(&"shader-pipeline") {
        toml.push_str(
            r#"
[build-dependencies]
ply-engine = { git = "https://github.com/TheRedDeveloper/ply-engine", features = ["shader-build"] }
"#,
        );
    }

    toml
}

pub(crate) const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{{TITLE}}</title>
    <style>
        html,
        body,
        canvas {
            margin: 0;
            padding: 0;
            width: 100%;
            height: 100%;
            overflow: hidden;
            position: absolute;
            background: black;
            z-index: 0;
        }
    </style>
</head>
<body>
    <canvas id="glcanvas" tabindex="0"></canvas>
    <script src="ply_bundle.js"></script>
    <script>load("app.wasm");</script>
</body>
</html>
"#;

pub(crate) fn generate_main_rs(font_filename: &str) -> String {
    format!(
        r#"use macroquad::prelude::*;
use ply_engine::{{
    grow, Ply,
    layout::{{LayoutAlignmentX, LayoutAlignmentY, LayoutDirection}},
}};

fn window_conf() -> macroquad::conf::Conf {{
    macroquad::conf::Conf {{
        miniquad_conf: miniquad::conf::Conf {{
            window_title: "Hello Ply!".to_owned(),
            window_width: 800,
            window_height: 600,
            high_dpi: true,
            sample_count: 4,
            platform: miniquad::conf::Platform {{
                webgl_version: miniquad::conf::WebGLVersion::WebGL2,
                ..Default::default()
            }},
            ..Default::default()
        }},
        draw_call_vertex_capacity: 100000,
        draw_call_index_capacity: 100000,
        ..Default::default()
    }}
}}

#[macroquad::main(window_conf)]
async fn main() {{
    let fonts = vec![load_ttf_font("assets/fonts/{font_filename}").await.unwrap()];
    let mut ply = Ply::<()>::new(fonts);

    loop {{
        clear_background(BLACK);

        let mut ui = ply.begin();

        ui.element().width(grow!()).height(grow!())
            .layout(|l| l
                .direction(LayoutDirection::TopToBottom)
                .gap(16)
                .align(LayoutAlignmentX::Center, LayoutAlignmentY::Center)
            )
            .children(|ui| {{
                ui.text("Hello, Ply!", |t| t
                    .font_size(32)
                    .color(0xFFFFFF)
                );
            }});

        ui.show(|_| {{}}).await;

        next_frame().await;
    }}
}}
"#
    )
}