//! Bounded background image decoding for the image-preview tabs.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::SystemTime;

use eframe::egui;

pub const MAX_IMAGE_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_ALLOC_BYTES: u64 = 128 * 1024 * 1024;
pub const HEX_WINDOW_BYTES: u64 = 2 * 1024 * 1024;

/// Formats that should open in the bounded hex viewer even when their bytes
/// happen to be valid UTF-8 text.
pub fn is_known_binary_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "bin"
                    | "pdb"
                    | "pak"
                    | "wad"
                    | "exe"
                    | "dll"
                    | "so"
                    | "dylib"
                    | "o"
                    | "obj"
                    | "class"
                    | "dat"
                    | "db"
                    | "sqlite"
                    | "sqlite3"
            )
        })
}

pub fn is_known_text_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "txt"
                    | "text"
                    | "log"
                    | "csv"
                    | "tsv"
                    | "json"
                    | "jsonc"
                    | "xml"
                    | "yaml"
                    | "yml"
                    | "toml"
                    | "ini"
                    | "cfg"
                    | "conf"
                    | "md"
                    | "markdown"
                    | "sql"
                    | "html"
                    | "htm"
                    | "css"
                    | "scss"
                    | "sass"
                    | "js"
                    | "mjs"
                    | "cjs"
                    | "ts"
                    | "tsx"
                    | "jsx"
                    | "rs"
                    | "py"
                    | "java"
                    | "kt"
                    | "go"
                    | "c"
                    | "h"
                    | "cpp"
                    | "hpp"
                    | "cs"
                    | "php"
                    | "rb"
                    | "lua"
                    | "sh"
                    | "bat"
                    | "ps1"
                    | "r"
                    | "swift"
                    | "vue"
                    | "svelte"
                    | "dockerfile"
                    | "makefile"
            )
        })
}

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub format: String,
}

pub struct ImagePreview {
    pub path: PathBuf,
    pub file_size: u64,
    pub receiver: Option<Receiver<Result<DecodedImage, String>>>,
    pub decoded: Option<DecodedImage>,
    pub texture: Option<egui::TextureHandle>,
    pub dimensions: Option<(u32, u32)>,
    pub format: Option<String>,
    pub error: Option<String>,
    pub fit: bool,
    pub zoom: f32,
}

pub struct HexPreview {
    pub path: PathBuf,
    pub file_size: u64,
    pub original_modified: Option<SystemTime>,
    pub receiver: Option<Receiver<Result<Vec<u8>, String>>>,
    pub bytes: Option<Vec<u8>>,
    pub error: Option<String>,
    pub search: String,
    pub selected_byte: Option<usize>,
    pub edit_value: String,
    pub edit_mode: bool,
    pub modified: bool,
}

impl HexPreview {
    pub fn start(path: &Path) -> Result<HexPreview, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        let path = path.to_path_buf();
        let worker_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("raven-hex-preview".into())
            .spawn(move || {
                let result = (|| {
                    let file =
                        std::fs::File::open(&worker_path).map_err(|error| error.to_string())?;
                    let mut bytes = Vec::with_capacity(HEX_WINDOW_BYTES as usize);
                    file.take(HEX_WINDOW_BYTES)
                        .read_to_end(&mut bytes)
                        .map_err(|error| error.to_string())?;
                    Ok(bytes)
                })();
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Could not start binary reader: {error}"))?;

        Ok(HexPreview {
            path,
            file_size: metadata.len(),
            original_modified: metadata.modified().ok(),
            receiver: Some(receiver),
            bytes: None,
            error: None,
            search: String::new(),
            selected_byte: None,
            edit_value: String::new(),
            edit_mode: false,
            modified: false,
        })
    }
}

impl ImagePreview {
    pub fn start(path: &Path) -> Result<ImagePreview, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_IMAGE_FILE_BYTES {
            return Err(format!(
                "Image preview is limited to {} MiB; this file is {} MiB",
                MAX_IMAGE_FILE_BYTES / (1024 * 1024),
                metadata.len() / (1024 * 1024)
            ));
        }
        let path = path.to_path_buf();
        let worker_path = path.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("raven-image-preview".into())
            .spawn(move || {
                let result = decode_image(&worker_path).map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Could not start image decoder: {error}"))?;

        Ok(ImagePreview {
            path,
            file_size: metadata.len(),
            receiver: Some(receiver),
            decoded: None,
            texture: None,
            dimensions: None,
            format: None,
            error: None,
            fit: true,
            zoom: 1.0,
        })
    }
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg"
            )
        })
}

pub fn decode_image(path: &Path) -> image::ImageResult<DecodedImage> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "svg" {
        return decode_svg(path);
    }

    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    reader.limits(limits);
    let decoded = reader.decode()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > MAX_IMAGE_PIXELS {
        return Err(image::ImageError::Limits(
            image::error::LimitError::from_kind(image::error::LimitErrorKind::InsufficientMemory),
        ));
    }
    Ok(DecodedImage {
        width,
        height,
        rgba: decoded.into_raw(),
        format: extension.to_ascii_uppercase(),
    })
}

fn decode_svg(path: &Path) -> image::ImageResult<DecodedImage> {
    let data = std::fs::read(path).map_err(image::ImageError::IoError)?;
    let options = usvg::Options::default();
    // Parse from bytes without a resource resolver: external URLs and linked
    // resources are not fetched as part of an untrusted SVG preview.
    let tree = usvg::Tree::from_data(&data, &options).map_err(|error| {
        image::ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    })?;
    let size = tree.size().to_int_size();
    let width = size.width();
    let height = size.height();
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width > 16_384 || height > 16_384 || pixels > MAX_IMAGE_PIXELS {
        return Err(image::ImageError::Limits(
            image::error::LimitError::from_kind(image::error::LimitErrorKind::InsufficientMemory),
        ));
    }
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        image::ImageError::Limits(image::error::LimitError::from_kind(
            image::error::LimitErrorKind::InsufficientMemory,
        ))
    })?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Ok(DecodedImage {
        width,
        height,
        rgba: pixmap.data().to_vec(),
        format: "SVG".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_requested_binary_extensions() {
        for extension in ["pdb", "pak", "wad", "bin"] {
            assert!(is_known_binary_path(
                Path::new("asset").with_extension(extension).as_path()
            ));
        }
    }

    #[test]
    fn sql_is_text_and_ico_is_an_image() {
        assert!(is_known_text_path(Path::new("query.sql")));
        assert!(is_image_path(Path::new("icon.ico")));
    }
}
