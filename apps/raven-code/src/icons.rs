//! Brand icon loading: the official crow SVG rasterized at build-asset
//! generation time (see scripts/gen_brand_assets.py) and embedded into the
//! binary. The SVG itself ships alongside as the brand source of truth.

use eframe::egui;

use std::path::Path;
use std::sync::Arc;

/// 32/64/128 px renders of the official logo, embedded at compile time so
/// the executable is self-contained (the 16/24/48 px variants exist for the
/// Windows .ico resource).
const ICON_64: &[u8] = include_bytes!("../assets/icons/raven-code-64.png");
const ICON_128: &[u8] = include_bytes!("../assets/icons/raven-code-128.png");

const FOLDER: &[u8] = include_bytes!("../assets/symbols/folders/folder.svg");
const FOLDER_OPEN: &[u8] = include_bytes!("../assets/symbols/folders/folder-open.svg");
const CC0_PLUS: &[u8] = include_bytes!("../assets/cc0-game-icons/plus.svg");
const FILETYPE_WORKSPACE: &[u8] = include_bytes!("../assets/file-types/workspace.svg");
const FILETYPE_CODE: &[u8] = include_bytes!("../assets/file-types/code.svg");
const FILETYPE_BINARY: &[u8] = include_bytes!("../assets/file-types/binary.svg");
const FILETYPE_OTHER: &[u8] = include_bytes!("../assets/file-types/hash.svg");

/// Decodes an embedded PNG into egui's icon format.
fn decode(png: &[u8]) -> egui::IconData {
    let image = image::load_from_memory(png).expect("embedded brand icon must decode");
    let rgba = image.to_rgba8();
    egui::IconData {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}

/// The window/taskbar icon (largest embedded render).
pub fn window_icon() -> Arc<egui::IconData> {
    Arc::new(decode(ICON_128))
}

/// Loads (or reuses) the brand mark texture for in-app display (About box).
pub fn brand_texture(ctx: &egui::Context) -> egui::TextureHandle {
    if let Some(handle) =
        ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(egui::Id::new("raven-brand-icon")))
    {
        return handle;
    }
    let icon = decode(ICON_64);
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    let handle = ctx.load_texture("raven-brand-icon", image, Default::default());
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("raven-brand-icon"), handle.clone());
    });
    handle
}

/// File category icon from the user-supplied Raven icon pack.
pub fn file_texture(ctx: &egui::Context, path: &Path) -> egui::TextureHandle {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rvenc"))
    {
        return svg_texture(ctx, "filetype-workspace", FILETYPE_WORKSPACE);
    }
    if crate::file_previews::is_known_binary_path(path) {
        return svg_texture(ctx, "filetype-binary", FILETYPE_BINARY);
    }
    if !crate::file_previews::is_known_text_path(path)
        && raven_editor::language::for_path(path).id == "plaintext"
    {
        return svg_texture(ctx, "filetype-other", FILETYPE_OTHER);
    }
    svg_texture(ctx, "filetype-code", FILETYPE_CODE)
}

pub fn folder_texture(ctx: &egui::Context, open: bool) -> egui::TextureHandle {
    svg_texture(
        ctx,
        if open { "folder-open" } else { "folder" },
        if open { FOLDER_OPEN } else { FOLDER },
    )
}

/// Small CC0 action icons bundled from Nieobie's Game Icon Pack.
pub fn add_file_texture(ctx: &egui::Context) -> egui::TextureHandle {
    svg_texture(ctx, "cc0-add-file", CC0_PLUS)
}

fn svg_texture(ctx: &egui::Context, key: &str, source: &[u8]) -> egui::TextureHandle {
    let id = egui::Id::new(("raven-symbol-icon", key, source.as_ptr() as usize));
    if let Some(texture) = ctx.data_mut(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(source, &options).expect("bundled Symbols SVG should parse");
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .expect("bundled Symbols SVG dimensions should be valid");
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let image = egui::ColorImage::from_rgba_premultiplied(
        [size.width() as usize, size.height() as usize],
        pixmap.data(),
    );
    let texture = ctx.load_texture(
        format!("raven-symbol-{key}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}
