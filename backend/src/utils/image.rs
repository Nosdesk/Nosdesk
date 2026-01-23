use image::{ImageFormat, ImageReader};
use tokio::fs;
use std::io::Cursor;
use tracing::{debug, warn, error};

/// Process and resize an uploaded avatar image to WebP format with fixed dimensions
/// This ensures consistent sizing and optimal storage
pub async fn process_avatar_image(
    image_bytes: &[u8],
    user_uuid: &str,
    max_size: u32, // Maximum width/height in pixels
) -> Result<Option<String>, String> {
    debug!(user_uuid = %user_uuid, max_size, "Processing avatar image");

    // Process image in a blocking task to avoid blocking the async runtime
    let user_uuid = user_uuid.to_string();
    let image_bytes = image_bytes.to_vec();
    let avatar_result = tokio::task::spawn_blocking(move || {
        // Load the image with EXIF orientation support
        let img = match load_image_with_orientation(&image_bytes) {
            Ok(img) => img,
            Err(e) => {
                error!(error = %e, "Failed to load image");
                return None;
            }
        };

        debug!(width = img.width(), height = img.height(), "Original image dimensions after orientation");

        // Create a square image by center cropping to 1:1 aspect ratio
        let square_img = create_square_crop(&img, max_size);

        debug!(width = square_img.width(), height = square_img.height(), "Final image dimensions");

        // Convert to WebP format
        let mut webp_bytes = Vec::new();
        match square_img.write_to(&mut std::io::Cursor::new(&mut webp_bytes), ImageFormat::WebP) {
            Ok(_) => {
                debug!("Successfully converted image to WebP format");
                Some(webp_bytes)
            },
            Err(e) => {
                error!(error = %e, "Failed to encode image as WebP");
                None
            }
        }
    }).await;
    
    let webp_bytes = match avatar_result {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(e) => {
            error!(error = %e, "Avatar processing task failed");
            return Ok(None);
        }
    };
    
    // Create avatar directory using storage abstraction
    let avatar_dir = "users/avatars";
    
    // Clean up any existing avatars for this user
    cleanup_old_user_avatars(avatar_dir, &user_uuid).await?;
    
    // Save the processed avatar using storage abstraction
    let avatar_filename = format!("{user_uuid}_avatar.webp");
    let avatar_path = format!("{avatar_dir}/{avatar_filename}");
    
    // For now, use direct filesystem access since storage abstraction needs to be passed in
    // TODO: Refactor to accept storage instance as parameter
    let full_avatar_path = format!("uploads/{avatar_path}");
    if let Err(e) = fs::create_dir_all(format!("uploads/{avatar_dir}")).await {
        return Err(format!("Failed to create avatar directory: {e}"));
    }
    
    match fs::write(&full_avatar_path, &webp_bytes).await {
        Ok(_) => {
            debug!(path = %full_avatar_path, "Successfully saved processed avatar");
            let avatar_url = format!("/uploads/{avatar_path}");
            Ok(Some(avatar_url))
        },
        Err(e) => {
            error!(error = %e, "Failed to save processed avatar");
            Ok(None)
        }
    }
}

/// Generate a 48x48 WebP thumbnail from an existing image file
pub async fn generate_user_avatar_thumbnail(
    image_path: &str,
    user_uuid: &str,
) -> Result<Option<String>, String> {
    // Convert URL path to file system path if needed
    let file_path = if image_path.starts_with('/') {
        format!(".{image_path}") // Remove leading slash and add current directory
    } else {
        image_path.to_string()
    };
    
    debug!(file_path = %file_path, "Generating thumbnail");
    
    // Read the original image
    let img_bytes = match fs::read(&file_path).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(file_path = %file_path, error = %e, "Failed to read avatar file");
            return Ok(None);
        }
    };
    
    // Process image in a blocking task to avoid blocking the async runtime
    let user_uuid = user_uuid.to_string();
    let thumbnail_result = tokio::task::spawn_blocking(move || {
        // Load the image with EXIF orientation support
        let img = match load_image_with_orientation(&img_bytes) {
            Ok(img) => img,
            Err(e) => {
                error!(error = %e, "Failed to load image");
                return None;
            }
        };

        // Create a square thumbnail by center cropping to 1:1 aspect ratio
        let thumbnail = create_square_crop(&img, 48);
        
        // Convert to WebP format
        let mut webp_bytes = Vec::new();
        match thumbnail.write_to(&mut std::io::Cursor::new(&mut webp_bytes), ImageFormat::WebP) {
            Ok(_) => Some(webp_bytes),
            Err(e) => {
                error!(error = %e, "Failed to encode image as WebP");
                None
            }
        }
    }).await;
    
    let webp_bytes = match thumbnail_result {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(e) => {
            error!(error = %e, "Thumbnail generation task failed");
            return Ok(None);
        }
    };
    
    // Create thumbnail directory
    let thumb_dir = "users/thumbs";
    let full_thumb_dir = format!("uploads/{thumb_dir}");
    if let Err(e) = fs::create_dir_all(&full_thumb_dir).await {
        return Err(format!("Failed to create thumbnail directory: {e}"));
    }
    
    // Clean up any existing thumbnails for this user
    cleanup_old_user_thumbnails(&full_thumb_dir, &user_uuid).await?;
    
    // Save the thumbnail
    let thumb_filename = format!("{user_uuid}_thumb.webp");
    let thumb_path = format!("{thumb_dir}/{thumb_filename}");
    let full_thumb_path = format!("uploads/{thumb_path}");
    
    match fs::write(&full_thumb_path, &webp_bytes).await {
        Ok(_) => {
            debug!(path = %full_thumb_path, "Successfully saved thumbnail");
            let thumb_url = format!("/uploads/{thumb_path}");
            Ok(Some(thumb_url))
        },
        Err(e) => {
            error!(error = %e, "Failed to save thumbnail");
            Ok(None)
        }
    }
}

/// Clean up old avatar files for a specific user
async fn cleanup_old_user_avatars(
    avatar_dir: &str,
    user_uuid: &str,
) -> Result<(), String> {
    // Read the directory
    let mut dir = match fs::read_dir(avatar_dir).await {
        Ok(dir) => dir,
        Err(_) => return Ok(()), // Directory doesn't exist, nothing to clean
    };

    // Look for files matching the pattern: {user_uuid}_avatar.{ext}
    let pattern_prefix = format!("{user_uuid}_avatar");
    
    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            // Check if this file matches our pattern (user_uuid_avatar.ext)
            if filename.starts_with(&pattern_prefix) && filename.contains('.') {
                let file_path = entry.path();
                debug!(file_path = ?file_path, "Cleaning up old avatar file");
                
                if let Err(e) = fs::remove_file(&file_path).await {
                    warn!(file_path = ?file_path, error = %e, "Failed to remove old avatar file");
                    // Continue with cleanup even if one file fails
                }
            }
        }
    }
    
    Ok(())
}

/// Clean up old thumbnail files for a specific user
async fn cleanup_old_user_thumbnails(
    thumb_dir: &str,
    user_uuid: &str,
) -> Result<(), String> {
    // Read the directory
    let mut dir = match fs::read_dir(thumb_dir).await {
        Ok(dir) => dir,
        Err(_) => return Ok(()), // Directory doesn't exist, nothing to clean
    };

    // Look for files matching the pattern: {user_uuid}_thumb.{ext}
    let pattern_prefix = format!("{user_uuid}_thumb");
    
    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            // Check if this file matches our pattern (user_uuid_thumb.ext)
            if filename.starts_with(&pattern_prefix) && filename.contains('.') {
                let file_path = entry.path();
                debug!(file_path = ?file_path, "Cleaning up old thumbnail file");
                
                if let Err(e) = fs::remove_file(&file_path).await {
                    warn!(file_path = ?file_path, error = %e, "Failed to remove old thumbnail file");
                    // Continue with cleanup even if one file fails
                }
            }
        }
    }
    
    Ok(())
}

/// Process and resize an uploaded banner image to WebP format with banner dimensions
/// This ensures consistent sizing and optimal storage for banner/cover images
pub async fn process_banner_image(
    image_bytes: &[u8],
    user_uuid: &str,
    max_width: u32,  // Maximum width in pixels (e.g., 1200)
    max_height: u32, // Maximum height in pixels (e.g., 400)
) -> Result<Option<String>, String> {
    debug!(user_uuid = %user_uuid, max_width, max_height, "Processing banner image");

    // Process image in a blocking task to avoid blocking the async runtime
    let user_uuid = user_uuid.to_string();
    let image_bytes = image_bytes.to_vec();
    let banner_result = tokio::task::spawn_blocking(move || {
        // Load the image with EXIF orientation support
        let img = match load_image_with_orientation(&image_bytes) {
            Ok(img) => img,
            Err(e) => {
                error!(error = %e, "Failed to load image");
                return None;
            }
        };

        debug!(width = img.width(), height = img.height(), "Original banner dimensions after orientation");

        // Create a banner-aspect image by cropping and resizing
        let banner_img = create_banner_crop(&img, max_width, max_height);

        debug!(width = banner_img.width(), height = banner_img.height(), "Final banner dimensions");

        // Convert to WebP format
        let mut webp_bytes = Vec::new();
        match banner_img.write_to(&mut std::io::Cursor::new(&mut webp_bytes), ImageFormat::WebP) {
            Ok(_) => {
                debug!("Successfully converted banner to WebP format");
                Some(webp_bytes)
            },
            Err(e) => {
                error!(error = %e, "Failed to encode banner as WebP");
                None
            }
        }
    }).await;
    
    let webp_bytes = match banner_result {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(e) => {
            error!(error = %e, "Banner processing task failed");
            return Ok(None);
        }
    };
    
    // Create banner directory
    let banner_dir = "users/banners";
    let full_banner_dir = format!("uploads/{banner_dir}");
    if let Err(e) = fs::create_dir_all(&full_banner_dir).await {
        return Err(format!("Failed to create banner directory: {e}"));
    }
    
    // Clean up any existing banners for this user
    cleanup_old_user_banners(&full_banner_dir, &user_uuid).await?;
    
    // Save the processed banner
    let banner_filename = format!("{user_uuid}_banner.webp");
    let banner_path = format!("{banner_dir}/{banner_filename}");
    let full_banner_path = format!("uploads/{banner_path}");
    
    match fs::write(&full_banner_path, &webp_bytes).await {
        Ok(_) => {
            debug!(path = %full_banner_path, "Successfully saved processed banner");
            let banner_url = format!("/uploads/{banner_path}");
            Ok(Some(banner_url))
        },
        Err(e) => {
            error!(error = %e, "Failed to save processed banner");
            Ok(None)
        }
    }
}

/// Clean up old banner files for a specific user
async fn cleanup_old_user_banners(
    banner_dir: &str,
    user_uuid: &str,
) -> Result<(), String> {
    // Read the directory
    let mut dir = match fs::read_dir(banner_dir).await {
        Ok(dir) => dir,
        Err(_) => return Ok(()), // Directory doesn't exist, nothing to clean
    };

    // Look for files matching the pattern: {user_uuid}_banner.{ext}
    let pattern_prefix = format!("{user_uuid}_banner");
    
    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            // Check if this file matches our pattern (user_uuid_banner.ext)
            if filename.starts_with(&pattern_prefix) && filename.contains('.') {
                let file_path = entry.path();
                debug!(file_path = ?file_path, "Cleaning up old banner file");
                
                if let Err(e) = fs::remove_file(&file_path).await {
                    warn!(file_path = ?file_path, error = %e, "Failed to remove old banner file");
                    // Continue with cleanup even if one file fails
                }
            }
        }
    }
    
    Ok(())
}

/// Create a banner-aspect crop of an image optimized for banner/cover images
/// This creates a 3:1 aspect ratio by default, suitable for profile banners
fn create_banner_crop(img: &image::DynamicImage, max_width: u32, max_height: u32) -> image::DynamicImage {
    let original_width = img.width();
    let original_height = img.height();

    debug!(original_width, original_height, max_width, max_height, "Creating banner crop");

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

        debug!(from_width = crop_width, from_height = crop_height, to_width = final_width, to_height = final_height, "Resizing banner");
        (final_width, final_height)
    } else {
        (crop_width, crop_height)
    };
    
    // Resize the banner to the final dimensions
    if final_width != crop_width || final_height != crop_height {
        cropped_img.resize_exact(final_width, final_height, image::imageops::FilterType::Lanczos3)
    } else {
        cropped_img
    }
}

/// Create a square crop of an image by center cropping to achieve 1:1 aspect ratio
/// This ensures consistent square avatars regardless of the original image dimensions
fn create_square_crop(img: &image::DynamicImage, target_size: u32) -> image::DynamicImage {
    let original_width = img.width();
    let original_height = img.height();

    debug!(original_width, original_height, target_size, "Creating square crop");

    // Determine the size of the square crop from the original image
    let crop_size = std::cmp::min(original_width, original_height);

    // Calculate the top-left coordinates for center cropping
    let crop_x = (original_width - crop_size) / 2;
    let crop_y = (original_height - crop_size) / 2;

    debug!(crop_size, crop_x, crop_y, "Cropping square");

    // Create the square crop
    let cropped_img = img.crop_imm(crop_x, crop_y, crop_size, crop_size);

    // Resize the square crop to the target size
    let final_img = if crop_size != target_size {
        debug!(from_size = crop_size, to_size = target_size, "Resizing cropped square");
        cropped_img.resize_exact(target_size, target_size, image::imageops::FilterType::Lanczos3)
    } else {
        cropped_img
    };

    final_img
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

    // Get the EXIF orientation (defaults to no rotation if not present)
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);

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