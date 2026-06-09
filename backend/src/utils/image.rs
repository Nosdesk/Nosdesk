use image::{ImageFormat, ImageReader};
use std::io::Cursor;
use tracing::{debug, error};

use crate::utils::storage::process_storage;

/// AUD-010: cap decode-time pixel dimensions so a tiny encoded
/// image can't expand into a multi-gigabyte raster.
///
/// 16384 covers any realistic photo or banner (8K is 7680 wide; an
/// ultrawide banner crop tops out around 12k). The `image` crate
/// treats width/height limits as STRICT, so a header claiming
/// 100_000 × 100_000 fails before any allocation happens.
///
/// `max_alloc` tightens the crate's default 512 MiB cap down to
/// 256 MiB. The default's non-strict, but for the decoders we
/// actually ship (PNG / JPEG / WebP via the image-rs defaults)
/// it's honored.
const IMAGE_MAX_DIMENSION: u32 = 16_384;
const IMAGE_MAX_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(IMAGE_MAX_DIMENSION);
    limits.max_image_height = Some(IMAGE_MAX_DIMENSION);
    limits.max_alloc = Some(IMAGE_MAX_ALLOC_BYTES);
    limits
}

/// Encode a decoded image as WebP. The `image` crate's `webp` feature
/// ships a lossless (VP8L) encoder, which `write_to` dispatches to.
fn encode_webp(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::WebP)
        .map_err(|e| format!("WebP encode failed: {e}"))?;
    Ok(out)
}

/// Normalise an avatar/banner reference to the logical key the `Storage`
/// backend expects. The reference may be a public URL
/// (`/uploads/users/avatars/x.webp`) coming back from a previous upload
/// or a bare storage path; both reduce to `users/avatars/x.webp`.
fn url_to_storage_path(reference: &str) -> String {
    reference
        .strip_prefix("/uploads/")
        .or_else(|| reference.strip_prefix("uploads/"))
        .unwrap_or(reference)
        .trim_start_matches('/')
        .to_string()
}

/// Process and resize an uploaded avatar image to a square lossless WebP.
///
/// Storage-backend agnostic: the processed bytes are written through the
/// process-wide [`process_storage`] handle, so the same code path works
/// for local disk and S3/Tigris. The key is deterministic
/// (`users/avatars/{uuid}_avatar.webp`) so a re-upload overwrites in
/// place — no directory-scan cleanup, which object stores can't do.
pub async fn process_avatar_image(
    image_bytes: &[u8],
    user_uuid: &str,
    max_size: u32, // Maximum width/height in pixels
) -> Result<Option<String>, String> {
    debug!(user_uuid = %user_uuid, max_size, "Processing avatar image");

    // CPU-bound decode/crop/encode on the blocking pool.
    let image_bytes = image_bytes.to_vec();
    let render = tokio::task::spawn_blocking(move || {
        let img = load_image_with_orientation(&image_bytes)?;
        let square_img = create_square_crop(&img, max_size);
        encode_webp(&square_img)
    })
    .await;

    let webp_bytes = match render {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            error!(user_uuid = %user_uuid, error = %e, "Failed to render avatar");
            return Ok(None);
        }
        Err(e) => {
            error!(user_uuid = %user_uuid, error = %e, "Avatar processing task panicked");
            return Ok(None);
        }
    };

    let path = format!("users/avatars/{user_uuid}_avatar.webp");
    match process_storage()
        .put_file(&webp_bytes, &path, "image/webp")
        .await
    {
        Ok(stored) => {
            debug!(user_uuid = %user_uuid, url = %stored.url, "Stored avatar via storage backend");
            Ok(Some(stored.url))
        }
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Failed to store avatar");
            Ok(None)
        }
    }
}

/// Render a square WebP thumbnail from raw image bytes. Used at
/// asset-media upload time; `size` is the output edge length in px.
pub async fn generate_asset_media_thumbnail(image_bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let bytes = image_bytes.to_vec();
    let render = tokio::task::spawn_blocking(move || {
        let img = load_image_with_orientation(&bytes)?;
        let square = create_square_crop(&img, size);
        encode_webp(&square)
    })
    .await;

    match render {
        Ok(Ok(webp)) => Some(webp),
        Ok(Err(e)) => {
            error!(error = %e, "Failed to render asset media thumbnail");
            None
        }
        Err(e) => {
            error!(error = %e, "Asset media thumbnail task panicked");
            None
        }
    }
}

/// Generate a 48x48 lossless WebP thumbnail from a user's stored avatar.
///
/// `image_ref` is the avatar's public URL or storage path; the source
/// bytes are read back through the storage backend (works for local and
/// S3) and the thumbnail is written to the deterministic
/// `users/thumbs/{uuid}_thumb.webp` key.
pub async fn generate_user_avatar_thumbnail(
    image_ref: &str,
    user_uuid: &str,
) -> Result<Option<String>, String> {
    let storage = process_storage();
    let source_path = url_to_storage_path(image_ref);
    debug!(source_path = %source_path, "Generating thumbnail from stored avatar");

    let img_bytes = match storage.get_file(&source_path).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(source_path = %source_path, error = ?e, "Failed to read avatar from storage");
            return Ok(None);
        }
    };

    let render = tokio::task::spawn_blocking(move || {
        let img = load_image_with_orientation(&img_bytes)?;
        let thumbnail = create_square_crop(&img, 48);
        encode_webp(&thumbnail)
    })
    .await;

    let webp_bytes = match render {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            error!(error = %e, "Failed to render thumbnail");
            return Ok(None);
        }
        Err(e) => {
            error!(error = %e, "Thumbnail generation task panicked");
            return Ok(None);
        }
    };

    let path = format!("users/thumbs/{user_uuid}_thumb.webp");
    match storage.put_file(&webp_bytes, &path, "image/webp").await {
        Ok(stored) => {
            debug!(user_uuid = %user_uuid, url = %stored.url, "Stored thumbnail via storage backend");
            Ok(Some(stored.url))
        }
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Failed to store thumbnail");
            Ok(None)
        }
    }
}

/// Process and resize an uploaded banner image to a lossless WebP at
/// banner dimensions. Storage-backend agnostic (see
/// [`process_avatar_image`]); the deterministic
/// `users/banners/{uuid}_banner.webp` key overwrites any previous banner
/// in place.
pub async fn process_banner_image(
    image_bytes: &[u8],
    user_uuid: &str,
    max_width: u32,  // Maximum width in pixels (e.g., 1200)
    max_height: u32, // Maximum height in pixels (e.g., 400)
) -> Result<Option<String>, String> {
    debug!(user_uuid = %user_uuid, max_width, max_height, "Processing banner image");

    let image_bytes = image_bytes.to_vec();
    let render = tokio::task::spawn_blocking(move || {
        let img = load_image_with_orientation(&image_bytes)?;
        let banner_img = create_banner_crop(&img, max_width, max_height);
        encode_webp(&banner_img)
    })
    .await;

    let webp_bytes = match render {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            error!(user_uuid = %user_uuid, error = %e, "Failed to render banner");
            return Ok(None);
        }
        Err(e) => {
            error!(user_uuid = %user_uuid, error = %e, "Banner processing task panicked");
            return Ok(None);
        }
    };

    let path = format!("users/banners/{user_uuid}_banner.webp");
    match process_storage()
        .put_file(&webp_bytes, &path, "image/webp")
        .await
    {
        Ok(stored) => {
            debug!(user_uuid = %user_uuid, url = %stored.url, "Stored banner via storage backend");
            Ok(Some(stored.url))
        }
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Failed to store banner");
            Ok(None)
        }
    }
}

/// Create a banner-aspect crop of an image optimized for banner/cover images
/// This creates a 3:1 aspect ratio by default, suitable for profile banners
fn create_banner_crop(
    img: &image::DynamicImage,
    max_width: u32,
    max_height: u32,
) -> image::DynamicImage {
    let original_width = img.width();
    let original_height = img.height();

    debug!(
        original_width,
        original_height, max_width, max_height, "Creating banner crop"
    );

    // Calculate the target aspect ratio
    let target_ratio = max_width as f32 / max_height as f32;
    let original_ratio = original_width as f32 / original_height as f32;

    let (crop_width, crop_height, crop_x, crop_y) = if original_ratio > target_ratio {
        // Image is too wide, crop horizontally
        let crop_height = original_height;
        let crop_width = (crop_height as f32 * target_ratio) as u32;
        let crop_x = (original_width - crop_width) / 2;
        let crop_y = 0;
        (crop_width, crop_height, crop_x, crop_y)
    } else {
        // Image is too tall, crop vertically
        let crop_width = original_width;
        let crop_height = (crop_width as f32 / target_ratio) as u32;
        let crop_x = 0;
        let crop_y = (original_height - crop_height) / 2;
        (crop_width, crop_height, crop_x, crop_y)
    };

    debug!(crop_width, crop_height, crop_x, crop_y, "Cropping banner");

    // Create the banner crop
    let cropped_img = img.crop_imm(crop_x, crop_y, crop_width, crop_height);

    // Resize to fit within the maximum dimensions while maintaining aspect ratio
    let (final_width, final_height) = if crop_width > max_width || crop_height > max_height {
        let scale_w = max_width as f32 / crop_width as f32;
        let scale_h = max_height as f32 / crop_height as f32;
        let scale = scale_w.min(scale_h);

        let final_width = (crop_width as f32 * scale) as u32;
        let final_height = (crop_height as f32 * scale) as u32;

        debug!(
            from_width = crop_width,
            from_height = crop_height,
            to_width = final_width,
            to_height = final_height,
            "Resizing banner"
        );
        (final_width, final_height)
    } else {
        (crop_width, crop_height)
    };

    // Resize the banner to the final dimensions
    if final_width != crop_width || final_height != crop_height {
        cropped_img.resize_exact(
            final_width,
            final_height,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        cropped_img
    }
}

/// Create a square crop of an image by center cropping to achieve 1:1 aspect ratio
/// This ensures consistent square avatars regardless of the original image dimensions
fn create_square_crop(img: &image::DynamicImage, target_size: u32) -> image::DynamicImage {
    let original_width = img.width();
    let original_height = img.height();

    debug!(
        original_width,
        original_height, target_size, "Creating square crop"
    );

    // Determine the size of the square crop from the original image
    let crop_size = std::cmp::min(original_width, original_height);

    // Calculate the top-left coordinates for center cropping
    let crop_x = (original_width - crop_size) / 2;
    let crop_y = (original_height - crop_size) / 2;

    debug!(crop_size, crop_x, crop_y, "Cropping square");

    // Create the square crop
    let cropped_img = img.crop_imm(crop_x, crop_y, crop_size, crop_size);

    // Resize the square crop to the target size
    if crop_size != target_size {
        debug!(
            from_size = crop_size,
            to_size = target_size,
            "Resizing cropped square"
        );
        cropped_img.resize_exact(
            target_size,
            target_size,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        cropped_img
    }
}

/// Load an image from bytes and apply EXIF orientation correction
/// This ensures images taken on phones/cameras display correctly regardless of how they were held
fn load_image_with_orientation(image_bytes: &[u8]) -> Result<image::DynamicImage, String> {
    use image::ImageDecoder;

    let cursor = Cursor::new(image_bytes);
    let reader = ImageReader::new(cursor)
        .with_guessed_format()
        .map_err(|e| format!("Failed to guess image format: {e}"))?;

    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("Failed to create decoder: {e}"))?;

    // AUD-010: enforce decode limits before any pixel allocation.
    // A bomb image with a pathological width/height in its header
    // fails here, before the decoder commits memory.
    decoder
        .set_limits(decode_limits())
        .map_err(|e| format!("Image exceeds decode limits: {e}"))?;

    // Get the EXIF orientation (defaults to no rotation if not present)
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);

    // Decode the image
    let mut img = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("Failed to decode image: {e}"))?;

    // Apply the orientation transformation
    img.apply_orientation(orientation);

    if orientation != image::metadata::Orientation::NoTransforms {
        debug!(orientation = ?orientation, "Applied EXIF orientation correction");
    }

    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn encode_png(width: u32, height: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
            .expect("encode test PNG");
        buf
    }

    #[test]
    fn encode_webp_roundtrips() {
        // Regression guard: the `image` crate's WebP *encoder* was
        // removed in 0.25, so `write_to(.., WebP)` silently fails at
        // runtime — which 500'd every avatar/banner upload. Assert our
        // `image-webp`-backed encoder produces bytes the decoder accepts.
        let png = encode_png(32, 24);
        let img = load_image_with_orientation(&png).expect("decode source");
        let webp = encode_webp(&img).expect("encode webp must succeed");

        assert!(
            webp.starts_with(b"RIFF"),
            "output must be a RIFF/WebP container"
        );
        let decoded = image::load_from_memory_with_format(&webp, ImageFormat::WebP)
            .expect("encoded webp must decode");
        assert_eq!(decoded.width(), 32);
        assert_eq!(decoded.height(), 24);
    }

    #[test]
    fn decode_limits_caps_dimensions_and_alloc() {
        let limits = decode_limits();
        assert_eq!(limits.max_image_width, Some(IMAGE_MAX_DIMENSION));
        assert_eq!(limits.max_image_height, Some(IMAGE_MAX_DIMENSION));
        assert_eq!(limits.max_alloc, Some(IMAGE_MAX_ALLOC_BYTES));
    }

    #[test]
    fn small_image_decodes_normally() {
        let png = encode_png(64, 64);
        let img = load_image_with_orientation(&png).expect("small image must decode");
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn image_at_the_limit_decodes() {
        // 16384 itself is allowed; only > 16384 should fail. The
        // image-rs `set_limits` semantics are <= the cap. Encoding a
        // full 16k x 16k image would be slow (megabytes of zeros);
        // a thinner test (a 16384 x 1 strip) exercises the boundary.
        let png = encode_png(IMAGE_MAX_DIMENSION, 1);
        load_image_with_orientation(&png).expect("image at the dimension limit must decode");
    }

    #[test]
    fn oversized_dimension_is_rejected() {
        // A real PNG that's one pixel wider than the limit. The
        // header carries the true dimension and the decoder refuses
        // before allocating the pixel buffer. This is the bomb
        // defence: an attacker can claim any dimension in the
        // header, and the strict width/height cap stops them at
        // header-parse time.
        let png = encode_png(IMAGE_MAX_DIMENSION + 1, 1);
        let err = load_image_with_orientation(&png)
            .expect_err("image past the dimension limit must be rejected");
        assert!(
            err.contains("decode limits") || err.to_lowercase().contains("limit"),
            "expected limit-related error, got: {err}"
        );
    }
}
