//! The app icon (assets/AppIcon-*.png, drawn by scripts/make-icon.swift) as a
//! GPUI image, for the toolbar and the Settings window.

use std::sync::{Arc, LazyLock};

use gpui_kit::*;

static SMALL: LazyLock<Arc<Image>> =
    LazyLock::new(|| png(include_bytes!("../../../assets/AppIcon-64.png")));
static LARGE: LazyLock<Arc<Image>> =
    LazyLock::new(|| png(include_bytes!("../../../assets/AppIcon-256.png")));

fn png(bytes: &[u8]) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()))
}

/// The icon at `size` points, from the 64px image up to 32pt and the 256px one
/// above that, so it isn't scaled down far enough to alias. The PNG includes
/// the macOS icon grid's margin, so the tile is about 80% of `size`.
pub fn app_icon(size: f32) -> Img {
    let image = if size <= 32. { &SMALL } else { &LARGE };
    img(Arc::clone(image)).size(px(size)).flex_none()
}
