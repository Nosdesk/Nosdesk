use image::ImageFormat;
use pdfium_render::prelude::{Pdfium, PdfRenderConfig, PdfPageRenderRotation};
use std::sync::OnceLock;
use tokio::fs;
use tracing::{debug, error, info, warn};

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

/// Check if PDF thumbnail generation is available
#[allow(dead_code)]
pub fn is_pdf_thumbnail_available() -> bool {
    check_pdfium_available()
}

/// Generate a WebP thumbnail from a PDF's first page
///
/// # Arguments
/// * `pdf_bytes` - The raw PDF file bytes
/// * `output_path` - Where to save the thumbnail (without extension, .webp will be added)
/// * `max_width` - Maximum width of the thumbnail
/// * `max_height` - Maximum height of the thumbnail
///
/// # Returns
/// * `Ok(Some(path))` - Thumbnail generated successfully, returns the file path
/// * `Ok(None)` - PDF rendering not available or PDF couldn't be processed
/// * `Err(e)` - An error occurred
pub async fn generate_pdf_thumbnail(
    pdf_bytes: &[u8],
    output_path: &str,
    max_width: u32,
    max_height: u32,
) -> Result<Option<String>, String> {
    if !check_pdfium_available() {
        debug!("Pdfium not available, skipping thumbnail generation");
        return Ok(None);
    }

    let pdf_bytes = pdf_bytes.to_vec();
    let output_path = output_path.to_string();

    // Process PDF in a blocking task to avoid blocking the async runtime
    let thumbnail_result = tokio::task::spawn_blocking(move || {
        generate_thumbnail_sync(&pdf_bytes, max_width, max_height)
    })
    .await
    .map_err(|e| format!("PDF thumbnail task panicked: {e}"))?;

    let webp_bytes = match thumbnail_result {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(e) => {
            error!(error = %e, "Failed to generate PDF thumbnail");
            return Ok(None);
        }
    };

    // Save the thumbnail
    let thumb_path = format!("{output_path}.webp");

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&thumb_path).parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return Err(format!("Failed to create thumbnail directory: {e}"));
        }
    }

    match fs::write(&thumb_path, &webp_bytes).await {
        Ok(_) => {
            debug!(path = %thumb_path, "Successfully saved PDF thumbnail");
            Ok(Some(thumb_path))
        }
        Err(e) => {
            error!(error = %e, path = %thumb_path, "Failed to save PDF thumbnail");
            Err(format!("Failed to save thumbnail: {e}"))
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

/// Generate a thumbnail for a PDF and store it alongside the original file
/// Returns the URL path to the thumbnail if successful
pub async fn generate_and_store_pdf_thumbnail(
    pdf_bytes: &[u8],
    original_file_path: &str,
    storage_base: &str,
) -> Result<Option<String>, String> {
    // Generate thumbnail path by replacing extension with _thumb.webp
    let thumb_path = original_file_path
        .strip_suffix(".pdf")
        .or_else(|| original_file_path.strip_suffix(".PDF"))
        .map(|base| format!("{base}_thumb"))
        .unwrap_or_else(|| format!("{original_file_path}_thumb"));

    let full_thumb_path = format!("{storage_base}/{thumb_path}");

    // Generate thumbnail (300px max width/height for grid view)
    match generate_pdf_thumbnail(pdf_bytes, &full_thumb_path, 300, 400).await? {
        Some(saved_path) => {
            // Convert filesystem path to URL path
            let url_path = saved_path
                .strip_prefix(storage_base)
                .map(|p| format!("/uploads{p}"))
                .unwrap_or_else(|| format!("/uploads/{thumb_path}.webp"));

            Ok(Some(url_path))
        }
        None => Ok(None),
    }
}
