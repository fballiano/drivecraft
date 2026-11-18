//! Renders the bundled SVG art (app icon, tray icons) to RGBA pixels.

pub const APP_ICON: &str = include_str!("../assets/app-icon.svg");
// Linux has no tray icon yet (see tray.rs).
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub const TRAY_IDLE: &str = include_str!("../assets/icons/cloud.svg");
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub const TRAY_SYNCING: &str = include_str!("../assets/icons/cloud-sync.svg");
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub const TRAY_ERROR: &str = include_str!("../assets/icons/cloud-alert.svg");
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub const TRAY_DONE: &str = include_str!("../assets/icons/cloud-check.svg");

/// Straight (not premultiplied) RGBA pixels of a square image.
pub struct Rgba {
    pub pixels: Vec<u8>,
    pub size: u32,
}

pub fn render(svg: &str, size: u32) -> Rgba {
    let pixels = pixmap(svg, size)
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Rgba { pixels, size }
}

fn pixmap(svg: &str, size: u32) -> resvg::tiny_skia::Pixmap {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("bundled SVG is valid");
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("icon size is not zero");
    let s = tree.size();
    let scale = size as f32 / s.width().max(s.height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

/// A Lucide icon in one color. Lucide icons draw with `currentColor`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn lucide(svg: &str, color: &str) -> String {
    svg.replace("currentColor", color)
}

#[cfg(test)]
mod tests {
    /// Writes the macOS icon set for the app bundle. To update the icon:
    ///
    /// ```text
    /// cargo test -p drivecraft app_icon_set -- --ignored
    /// iconutil -c icns target/DriveCraft.iconset -o crates/app/assets/DriveCraft.icns
    /// ```
    #[test]
    #[ignore = "writes PNG files; run on demand"]
    fn app_icon_set() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/DriveCraft.iconset");
        std::fs::create_dir_all(&dir).unwrap();
        for size in [16, 32, 128, 256, 512] {
            for (scale, suffix) in [(1, ""), (2, "@2x")] {
                let name = format!("icon_{size}x{size}{suffix}.png");
                super::pixmap(super::APP_ICON, size * scale)
                    .save_png(dir.join(name))
                    .unwrap();
            }
        }
    }
}
