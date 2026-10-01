use image::ImageFormat;
use pdfium_render::prelude::{PdfPageRenderRotation, PdfRenderConfig, Pdfium};
use std::sync::OnceLock;
use tracing::{debug, error, info, warn};

use crate::utils::storage::Storage;

/// Track whether pdfium is available (checked once at startup)
static PDFIUM_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Check if pdfium library is available on this system
fn check_pdfium_available() -> bool {
    *PDFIUM_AVAILABLE.get_or_init(|| {
        // Try to bind to the system pdfium library
        if Pdfium::bind_to_system_library().is_ok() {
            info!("Pdfium system library is available");
            return true;
        }

        // Try common paths for pdfium library
        let paths = [
            "./libpdfium.so",
            "/usr/lib/libpdfium.so",
            "/usr/local/lib/libpdfium.so",
            "./pdfium.dll",
            "./libpdfium.dylib",
        ];

        for path in paths {
            if Pdfium::bind_to_library(path).is_ok() {
                info!(path = %path, "Pdfium library found");
                return true;
            }
        }

        warn!(
            "Pdfium library not available - PDF thumbnails will be disabled. \
             Install pdfium or place libpdfium.so in the application directory."
        );
        false
    })
}

/// Create a new Pdfium instance (called per-operation for thread safety)
fn create_pdfium() -> Option<Pdfium> {
    // Try system library first
    if let Ok(bindings) = Pdfium::bind_to_system_library() {
        return Some(Pdfium::new(bindings));
    }

    // Try common paths
    let paths = [
        "./libpdfium.so",
        "/usr/lib/libpdfium.so",
        "/usr/local/lib/libpdfium.so",
        "./pdfium.dll",
        "./libpdfium.dylib",
    ];

    for path in paths {
        if let Ok(bindings) = Pdfium::bind_to_library(path) {
            return Some(Pdfium::new(bindings));
        }
    }

    None
}

/// Render a PDF's first page as WebP, within `max_width` x `max_height`.
///
/// `Ok(None)` when pdfium isn't available or the PDF can't be rendered.
async fn render_pdf_thumbnail(
    pdf_bytes: &[u8],
    max_width: u32,
    max_height: u32,
) -> Result<Option<Vec<u8>>, String> {
    if !check_pdfium_available() {
        debug!("Pdfium not available, skipping thumbnail generation");
        return Ok(None);
    }

    let pdf_bytes = pdf_bytes.to_vec();

    // Process PDF in a blocking task to avoid blocking the async runtime
    let thumbnail_result = tokio::task::spawn_blocking(move || {
        generate_thumbnail_sync(&pdf_bytes, max_width, max_height)
    })
    .await
    .map_err(|e| format!("PDF thumbnail task panicked: {e}"))?;

    match thumbnail_result {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            error!(error = %e, "Failed to generate PDF thumbnail");
            Ok(None)
        }
    }
}

/// Synchronous thumbnail generation (runs in blocking task)
fn generate_thumbnail_sync(
    pdf_bytes: &[u8],
    max_width: u32,
    max_height: u32,
) -> Result<Option<Vec<u8>>, String> {
    let pdfium = match create_pdfium() {
        Some(p) => p,
        None => return Ok(None),
    };

    // Load the PDF from bytes
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, None)
        .map_err(|e| format!("Failed to load PDF: {e}"))?;

    // Get the first page
    let page = document
        .pages()
        .get(0)
        .map_err(|e| format!("Failed to get first page: {e}"))?;

    // Configure rendering
    let render_config = PdfRenderConfig::new()
        .set_maximum_width(max_width as i32)
        .set_maximum_height(max_height as i32)
        .rotate_if_landscape(PdfPageRenderRotation::None, false);

    // Render the page to an image
    let image = page
        .render_with_config(&render_config)
        .map_err(|e| format!("Failed to render PDF page: {e}"))?
        .as_image();

    // Convert to RGB8 for WebP encoding
    let rgb_image = image.into_rgb8();

    debug!(
        width = rgb_image.width(),
        height = rgb_image.height(),
        "Rendered PDF page to image"
    );

    // Convert to WebP format
    let mut webp_bytes = Vec::new();
    let dynamic_image = image::DynamicImage::ImageRgb8(rgb_image);

    dynamic_image
        .write_to(
            &mut std::io::Cursor::new(&mut webp_bytes),
            ImageFormat::WebP,
        )
        .map_err(|e| format!("Failed to encode thumbnail as WebP: {e}"))?;

    debug!(size_bytes = webp_bytes.len(), "Generated PDF thumbnail");

    Ok(Some(webp_bytes))
}

/// What replaces a PDF's `.pdf` extension in its thumbnail's name. The frontend
/// derives a thumbnail URL from the attachment URL the same way.
const THUMB_SUFFIX: &str = "_thumb.webp";

/// Path of a PDF's thumbnail: `.pdf` (or `.PDF`) becomes `_thumb.webp`. `None`
/// for any other extension.
pub fn thumbnail_path(pdf_path: &str) -> Option<String> {
    pdf_path
        .strip_suffix(".pdf")
        .or_else(|| pdf_path.strip_suffix(".PDF"))
        .map(|base| format!("{base}{THUMB_SUFFIX}"))
}

/// The PDF paths a thumbnail path can belong to, the inverse of
/// [`thumbnail_path`]. Empty when the path isn't a thumbnail's.
pub fn pdf_paths_for_thumbnail(thumb_path: &str) -> Vec<String> {
    thumb_path
        .strip_suffix(THUMB_SUFFIX)
        .map(|base| vec![format!("{base}.pdf"), format!("{base}.PDF")])
        .unwrap_or_default()
}

/// Render a thumbnail of the PDF stored at `pdf_path` and store it beside the
/// PDF, at [`thumbnail_path`], through the same storage. Pass the request's
/// workspace-scoped storage so the thumbnail lands in the workspace's prefix and
/// moves with the PDF when it is attached. Returns the thumbnail's URL, or
/// `None` when no thumbnail could be rendered.
pub async fn store_pdf_thumbnail(
    storage: &dyn Storage,
    pdf_bytes: &[u8],
    pdf_path: &str,
) -> Result<Option<String>, String> {
    let Some(thumb_path) = thumbnail_path(pdf_path) else {
        return Ok(None);
    };
    // 300x400 max for the attachment grid.
    let Some(webp) = render_pdf_thumbnail(pdf_bytes, 300, 400).await? else {
        return Ok(None);
    };
    storage
        .put_file(&webp, &thumb_path, "image/webp")
        .await
        .map(|stored| Some(stored.url))
        .map_err(|e| format!("Failed to store thumbnail: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_path_replaces_the_pdf_extension() {
        assert_eq!(
            thumbnail_path("temp/0190_report.pdf").as_deref(),
            Some("temp/0190_report_thumb.webp")
        );
        assert_eq!(
            thumbnail_path("temp/0190_SCAN.PDF").as_deref(),
            Some("temp/0190_SCAN_thumb.webp")
        );
        assert_eq!(thumbnail_path("temp/0190_photo.png"), None);
    }

    #[test]
    fn pdf_paths_for_thumbnail_inverts_thumbnail_path() {
        for pdf in ["temp/0190_report.pdf", "temp/0190_SCAN.PDF"] {
            let thumb = thumbnail_path(pdf).expect("a pdf has a thumbnail path");
            assert!(pdf_paths_for_thumbnail(&thumb).contains(&pdf.to_string()));
        }
        assert!(pdf_paths_for_thumbnail("temp/0190_photo.png").is_empty());
    }
}
