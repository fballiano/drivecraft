//! Renders the bundled SVG art (app icon, tray icons) to RGBA pixels.

pub const APP_ICON: &str = include_str!("../assets/app-icon.svg");
pub const TRAY_IDLE: &str = include_str!("../assets/icons/cloud.svg");
pub const TRAY_SYNCING: &str = include_str!("../assets/icons/cloud-sync.svg");
pub const TRAY_ERROR: &str = include_str!("../assets/icons/cloud-alert.svg");
pub const TRAY_DONE: &str = include_str!("../assets/icons/cloud-check.svg");

/// Straight (not premultiplied) RGBA pixels of a square image.
pub struct Rgba {
    pub pixels: Vec<u8>,
    pub size: u32,
}

pub fn render(svg: &str, size: u32) -> Rgba {
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
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Rgba { pixels, size }
}

/// A Lucide icon in one color. Lucide icons draw with `currentColor`.
pub fn lucide(svg: &str, color: &str) -> String {
    svg.replace("currentColor", color)
}
