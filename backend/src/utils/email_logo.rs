//! Email-ready copies of a workspace's logos.
//!
//! Mail clients are stricter than browsers: desktop Outlook shows no WebP, and
//! a letterhead has to state its size up front or the client stretches the
//! image. So each logo upload also gets a PNG rendition for email, trimmed of
//! transparent margins and drawn at twice its display size for high-density
//! screens. The display size, and whether the logo stands out on the letter's
//! light and dark paper, are stored with it (`site_settings.email_logo`,
//! `email_logo_light`), so sending a message never decodes an image.

use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};

/// The box a logo is shown in at most, in CSS pixels.
pub const MAX_DISPLAY_WIDTH: u32 = 200;
pub const MAX_DISPLAY_HEIGHT: u32 = 48;

/// The letter's light and dark paper. Each is also the backing a logo sits
/// on in the other mode when it can't be seen there.
pub const PAPER_LIGHT: [u8; 3] = [0xf6, 0xf2, 0xea];
pub const PAPER_DARK: [u8; 3] = [0x0b, 0x0a, 0x08];

/// Below this APCA lightness contrast (`Lc`) a part of a logo is hard to
/// make out: APCA's floor for non-text marks. Navy on the dark paper is
/// about 9; an amber mark on the light paper, about 34.
const VISIBLE_LC: f64 = 15.0;
/// What the workspace's name needs when it is set as the letterhead (24 px
/// bold): APCA's level for large bold text. Nosdesk orange is about 46 on
/// the light paper and 48 on the dark.
const WORDMARK_LC: f64 = 45.0;
/// The share of a logo's ink that has to be visible on a paper for it to go
/// there bare. High, since a backing costs little and a vanished word costs
/// the logo.
const READS_SHARE: f64 = 0.9;
/// A logo that fills this much of its own box brings its own background (a
/// badge, a photo, an opaque JPEG), so it reads on any paper.
const SELF_BACKED_COVERAGE: f64 = 0.75;
/// Alpha at or below this is left out when trimming: antialiasing dust from
/// an export, not part of the mark.
const ALPHA_VISIBLE: u8 = 8;

/// A rendition as recorded on `site_settings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailLogo {
    /// Origin-relative URL of the PNG.
    pub url: String,
    /// Display size in CSS pixels. The PNG holds up to twice as many.
    pub width: u32,
    pub height: u32,
    /// Whether the logo stands out on the light and the dark paper without a
    /// backing.
    pub reads_on_light: bool,
    pub reads_on_dark: bool,
}

/// The rendered PNG and what the letterhead needs to know about it.
#[derive(Debug)]
pub struct Rendition {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub reads_on_light: bool,
    pub reads_on_dark: bool,
}

impl Rendition {
    /// The record to store once the PNG is saved at `url`.
    pub fn logo(&self, url: String) -> EmailLogo {
        EmailLogo {
            url,
            width: self.width,
            height: self.height,
            reads_on_light: self.reads_on_light,
            reads_on_dark: self.reads_on_dark,
        }
    }
}

/// Display and pixel size for a logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    /// CSS pixels: the source fitted into the box, never enlarged.
    pub display: (u32, u32),
    /// The PNG's own size: twice the display size, but never more than the
    /// source has.
    pub pixels: (u32, u32),
}

/// Size a `width` x `height` logo for the letterhead. A source smaller than
/// the box keeps its natural size at 1x; it is not halved to look like 2x.
pub fn fit(width: u32, height: u32) -> Fit {
    let (w, h) = (width.max(1) as f64, height.max(1) as f64);
    let scale = (MAX_DISPLAY_WIDTH as f64 / w)
        .min(MAX_DISPLAY_HEIGHT as f64 / h)
        .min(1.0);
    let pixel_scale = (scale * 2.0).min(1.0);
    let size = |s: f64| {
        (
            ((w * s).round() as u32).max(1),
            ((h * s).round() as u32).max(1),
        )
    };
    Fit {
        display: size(scale),
        pixels: size(pixel_scale),
    }
}

/// Make the email rendition of an uploaded logo (PNG, JPEG or WebP).
pub fn render(source: &[u8]) -> Result<Rendition, String> {
    let decoded = crate::utils::image::load_image_with_orientation(source)?;
    let image = trim_transparent(decoded.to_rgba8())?;
    let fit = fit(image.width(), image.height());
    let image = resize_premultiplied(&image, fit.pixels);

    let self_backed = coverage(&image) >= SELF_BACKED_COVERAGE;
    let opaque = image.pixels().all(|p| p[3] == u8::MAX);

    Ok(Rendition {
        png: encode_png(&image, opaque)?,
        width: fit.display.0,
        height: fit.display.1,
        reads_on_light: self_backed || reads_on(&image, PAPER_LIGHT),
        reads_on_dark: self_backed || reads_on(&image, PAPER_DARK),
    })
}

/// Crop away transparent margins, so the logo's own artwork sets its size.
/// An opaque image has none and comes back whole: its background is part of
/// the design.
fn trim_transparent(image: RgbaImage) -> Result<RgbaImage, String> {
    let (width, height) = image.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] > ALPHA_VISIBLE {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if left > right || top > bottom {
        return Err("The image is fully transparent".into());
    }
    if (left, top, right, bottom) == (0, 0, width - 1, height - 1) {
        return Ok(image);
    }
    Ok(image::imageops::crop_imm(&image, left, top, right - left + 1, bottom - top + 1).to_image())
}

/// Resample with alpha premultiplied, as `imageops::resize` expects. Resized
/// straight, the colour of fully transparent pixels (usually black) bleeds
/// into the edges and leaves a dark fringe around a light logo.
fn resize_premultiplied(image: &RgbaImage, (width, height): (u32, u32)) -> RgbaImage {
    if image.dimensions() == (width, height) {
        return image.clone();
    }
    let mut premultiplied = image.clone();
    for pixel in premultiplied.pixels_mut() {
        let alpha = pixel[3] as u32;
        for channel in &mut pixel.0[..3] {
            *channel = ((*channel as u32 * alpha + 127) / 255) as u8;
        }
    }
    let mut resized = image::imageops::resize(&premultiplied, width, height, FilterType::Lanczos3);
    for pixel in resized.pixels_mut() {
        let alpha = pixel[3] as u32;
        if alpha == 0 {
            pixel.0 = [0, 0, 0, 0];
            continue;
        }
        for channel in &mut pixel.0[..3] {
            *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    resized
}

/// The share of the box the logo covers near-opaquely.
fn coverage(image: &RgbaImage) -> f64 {
    let total = (image.width() * image.height()) as f64;
    let solid = image.pixels().filter(|p| p[3] >= 230).count() as f64;
    solid / total
}

/// Whether nearly all of the logo's ink, weighted by opacity, can be seen on
/// `paper`. Faint shadows and edge fuzz weigh little or nothing.
fn reads_on(image: &RgbaImage, paper: [u8; 3]) -> bool {
    let (mut ink, mut visible) = (0.0, 0.0);
    for pixel in image.pixels() {
        let alpha = pixel[3] as f64 / 255.0;
        if alpha < 0.25 {
            continue;
        }
        ink += alpha;
        if apca_contrast([pixel[0], pixel[1], pixel[2]], paper).abs() >= VISIBLE_LC {
            visible += alpha;
        }
    }
    ink > 0.0 && visible / ink >= READS_SHARE
}

fn encode_png(image: &RgbaImage, opaque: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder =
        PngEncoder::new_with_quality(&mut out, CompressionType::Best, PngFilter::Adaptive);
    let written = if opaque {
        let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
        encoder.write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
    } else {
        encoder.write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
    };
    written.map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(out)
}

// --- Colour ----------------------------------------------------------------

/// `#rgb` or `#rrggbb` to its channels.
pub fn parse_hex_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        3 => {
            let mut rgb = [0u8; 3];
            for (i, c) in hex.chars().enumerate() {
                let v = channel(&c.to_string())?;
                rgb[i] = v * 17;
            }
            Some(rgb)
        }
        6 => Some([
            channel(&hex[0..2])?,
            channel(&hex[2..4])?,
            channel(&hex[4..6])?,
        ]),
        _ => None,
    }
}

/// APCA lightness contrast (`Lc`, about -108 to 106) of `text` on
/// `background`, by the APCA-W3 0.0.98G formula: positive for dark on
/// light, negative for light on dark. Unlike the WCAG 2 ratio it tracks how
/// hard dark-on-dark is to see, which is what the dark paper has to judge.
pub fn apca_contrast(text: [u8; 3], background: [u8; 3]) -> f64 {
    fn luminance([r, g, b]: [u8; 3]) -> f64 {
        let channel = |c: u8| (c as f64 / 255.0).powf(2.4);
        let y = 0.2126729 * channel(r) + 0.7151522 * channel(g) + 0.0721750 * channel(b);
        // Soft clamp near black.
        if y < 0.022 {
            y + (0.022 - y).powf(1.414)
        } else {
            y
        }
    }
    let (text, background) = (luminance(text), luminance(background));
    if (background - text).abs() < 0.0005 {
        return 0.0;
    }
    if background > text {
        let s = (background.powf(0.56) - text.powf(0.57)) * 1.14;
        if s < 0.1 {
            0.0
        } else {
            (s - 0.027) * 100.0
        }
    } else {
        let s = (background.powf(0.65) - text.powf(0.62)) * 1.14;
        if s > -0.1 {
            0.0
        } else {
            (s + 0.027) * 100.0
        }
    }
}

/// WCAG 2 relative luminance.
pub fn relative_luminance([r, g, b]: [u8; 3]) -> f64 {
    let linear = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG 2 contrast ratio, 1 to 21.
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (light, dark) = if la > lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Black or white, whichever contrasts more with `fill`. The same rule as the
/// web app's `pickAccentForeground`, so a button reads the same in both.
pub fn text_on(fill: [u8; 3]) -> &'static str {
    if contrast_ratio(fill, [0xff; 3]) > contrast_ratio(fill, [0; 3]) {
        "#ffffff"
    } else {
        "#000000"
    }
}

/// Whether the workspace's name can be set in `color` on `paper`.
pub fn wordmark_reads_on(color: [u8; 3], paper: [u8; 3]) -> bool {
    apca_contrast(color, paper).abs() >= WORDMARK_LC
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba};

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    /// A logo of `ink` on a transparent canvas, with `margin` pixels of
    /// empty space on every side.
    fn mark(width: u32, height: u32, margin: u32, ink: [u8; 3]) -> RgbaImage {
        RgbaImage::from_fn(width + 2 * margin, height + 2 * margin, |x, y| {
            let inside = x >= margin && x < margin + width && y >= margin && y < margin + height;
            // Stripes, so the ink covers about half its box like a wordmark
            // does, with both end columns inked so the box trims to `width`.
            if inside && {
                let column = x - margin;
                column.is_multiple_of(2) || column == width - 1
            } {
                Rgba([ink[0], ink[1], ink[2], 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    fn decode(png: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(png, ImageFormat::Png)
            .unwrap()
            .to_rgba8()
    }

    #[test]
    fn a_large_logo_is_shown_in_the_box_and_drawn_at_twice_that() {
        assert_eq!(
            fit(1000, 200),
            Fit {
                display: (200, 40),
                pixels: (400, 80)
            }
        );
        // Height-bound: a square mark.
        assert_eq!(
            fit(512, 512),
            Fit {
                display: (48, 48),
                pixels: (96, 96)
            }
        );
    }

    #[test]
    fn a_small_logo_keeps_its_own_size() {
        assert_eq!(
            fit(100, 24),
            Fit {
                display: (100, 24),
                pixels: (100, 24)
            }
        );
        // Between 1x and 2x of the box: shown fitted, drawn at all it has.
        assert_eq!(
            fit(300, 60),
            Fit {
                display: (200, 40),
                pixels: (300, 60)
            }
        );
    }

    #[test]
    fn a_very_wide_or_tall_logo_keeps_its_proportions() {
        assert_eq!(
            fit(4000, 100),
            Fit {
                display: (200, 5),
                pixels: (400, 10)
            }
        );
        assert_eq!(
            fit(100, 400),
            Fit {
                display: (12, 48),
                pixels: (24, 96)
            }
        );
    }

    #[test]
    fn transparent_margins_are_trimmed_before_sizing() {
        // 400 x 80 of artwork inside 300 px of empty canvas on every side.
        let source = png(&mark(400, 80, 300, [0x20, 0x20, 0x20]));
        let rendition = render(&source).unwrap();
        assert_eq!((rendition.width, rendition.height), (200, 40));
        let out = decode(&rendition.png);
        assert_eq!(out.dimensions(), (400, 80), "drawn at 2x");
    }

    #[test]
    fn an_opaque_image_keeps_its_background() {
        // A JPEG has no transparency: its white margin is part of the logo.
        let mut canvas = RgbaImage::from_pixel(300, 100, Rgba([255, 255, 255, 255]));
        for x in 100..200 {
            for y in 40..60 {
                canvas.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgba8(canvas)
            .to_rgb8()
            .write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        let rendition = render(&jpeg).unwrap();
        assert_eq!((rendition.width, rendition.height), (144, 48));
        assert!(rendition.reads_on_light && rendition.reads_on_dark);
        let out = image::load_from_memory_with_format(&rendition.png, ImageFormat::Png).unwrap();
        assert!(!out.color().has_alpha(), "opaque output carries no alpha");
    }

    #[test]
    fn webp_and_jpeg_become_png() {
        let mut webp = Vec::new();
        image::DynamicImage::ImageRgba8(mark(120, 30, 4, [0x10, 0x10, 0x10]))
            .write_to(&mut std::io::Cursor::new(&mut webp), ImageFormat::WebP)
            .unwrap();
        let rendition = render(&webp).unwrap();
        assert!(rendition.png.starts_with(b"\x89PNG"));
        assert_eq!((rendition.width, rendition.height), (120, 30));
    }

    #[test]
    fn a_fully_transparent_image_is_refused() {
        let empty = RgbaImage::from_pixel(50, 50, Rgba([0, 0, 0, 0]));
        assert!(render(&png(&empty)).is_err());
    }

    #[test]
    fn garbage_is_refused() {
        assert!(render(b"not an image").is_err());
    }

    #[test]
    fn dark_ink_reads_on_light_paper_only() {
        let r = render(&png(&mark(200, 40, 0, [0x1e, 0x3a, 0x8a]))).unwrap();
        assert!(r.reads_on_light);
        assert!(!r.reads_on_dark);
    }

    #[test]
    fn white_ink_reads_on_dark_paper_only() {
        let r = render(&png(&mark(200, 40, 0, [0xff, 0xff, 0xff]))).unwrap();
        assert!(!r.reads_on_light);
        assert!(r.reads_on_dark);
    }

    #[test]
    fn nosdesk_orange_reads_on_both() {
        let r = render(&png(&mark(200, 40, 0, [0xff, 0x6b, 0x1a]))).unwrap();
        assert!(r.reads_on_light && r.reads_on_dark);
    }

    #[test]
    fn a_badge_brings_its_own_background() {
        // White tile with dark text and transparent rounded-off corners: the
        // tile is barely visible on the light paper, but the logo reads.
        let badge = RgbaImage::from_fn(100, 100, |x, y| {
            let corner = !(6..=93).contains(&x) && !(6..=93).contains(&y);
            if corner {
                Rgba([0, 0, 0, 0])
            } else if (40..60).contains(&y) {
                Rgba([0x11, 0x11, 0x11, 255])
            } else {
                Rgba([0xff, 0xff, 0xff, 255])
            }
        });
        let r = render(&png(&badge)).unwrap();
        assert!(r.reads_on_light && r.reads_on_dark);
    }

    #[test]
    fn resizing_leaves_no_dark_fringe_on_a_light_logo() {
        // A white disc on transparent black: straight-alpha resampling greys
        // its edge.
        let disc = RgbaImage::from_fn(400, 400, |x, y| {
            let (dx, dy) = (x as f64 - 199.5, y as f64 - 199.5);
            if dx * dx + dy * dy < 190.0 * 190.0 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let out = decode(&render(&png(&disc)).unwrap().png);
        for p in out.pixels().filter(|p| p[3] > 0) {
            assert!(
                p[0] >= 250 && p[1] >= 250 && p[2] >= 250,
                "fringe pixel {p:?}"
            );
        }
    }

    #[test]
    fn hex_colours_parse_in_both_lengths() {
        assert_eq!(parse_hex_color("#FF6B1A"), Some([0xff, 0x6b, 0x1a]));
        assert_eq!(parse_hex_color("#f60"), Some([0xff, 0x66, 0x00]));
        assert_eq!(parse_hex_color("FF6B1A"), None);
        assert_eq!(parse_hex_color("#GG0000"), None);
        assert_eq!(parse_hex_color("#ff6b1"), None);
    }

    #[test]
    fn button_text_follows_the_web_apps_rule() {
        // The web app's own examples: Nosdesk orange and Slate cyan take black,
        // black takes white.
        assert_eq!(text_on([0xff, 0x6b, 0x1a]), "#000000");
        assert_eq!(text_on([0x06, 0xb6, 0xd4]), "#000000");
        assert_eq!(text_on([0, 0, 0]), "#ffffff");
        // Either side of the crossover, where white and black contrast equally
        // (luminance ~0.179): #747474 is just darker, #767676 just lighter.
        assert_eq!(text_on([0x74, 0x74, 0x74]), "#ffffff");
        assert_eq!(text_on([0x76, 0x76, 0x76]), "#000000");
    }

    #[test]
    fn apca_matches_its_reference_values() {
        let close = |a: f64, b: f64| (a - b).abs() < 0.05;
        assert!(close(apca_contrast([0x88; 3], [0xff; 3]), 63.06));
        assert!(close(apca_contrast([0xff; 3], [0x88; 3]), -68.54));
        assert!(close(apca_contrast([0; 3], [0xff; 3]), 106.04));
        assert!(close(apca_contrast([0xff; 3], [0; 3]), -107.88));
    }

    #[test]
    fn a_colourful_mark_beside_dark_text_reads_on_light_paper() {
        // Navy lettering and an amber dot: a common two-colour logo. The dot
        // is only about 2:1 by WCAG on the light paper, but plainly visible.
        let logo = RgbaImage::from_fn(300, 60, |x, _| {
            if x < 60 {
                Rgba([0xf5, 0x9e, 0x0b, 255])
            } else if x.is_multiple_of(2) || x == 299 {
                Rgba([0x1e, 0x3a, 0x8a, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let r = render(&png(&logo)).unwrap();
        assert!(r.reads_on_light, "both colours show on the light paper");
        assert!(!r.reads_on_dark, "the lettering is lost on the dark paper");
    }

    #[test]
    fn a_name_is_set_in_its_colour_only_where_it_reads() {
        let orange = [0xff, 0x6b, 0x1a];
        assert!(wordmark_reads_on(orange, PAPER_LIGHT) && wordmark_reads_on(orange, PAPER_DARK));
        let navy = [0x1e, 0x3a, 0x8a];
        assert!(wordmark_reads_on(navy, PAPER_LIGHT) && !wordmark_reads_on(navy, PAPER_DARK));
        let yellow = [0xff, 0xd6, 0x00];
        assert!(!wordmark_reads_on(yellow, PAPER_LIGHT) && wordmark_reads_on(yellow, PAPER_DARK));
    }

    #[test]
    fn contrast_matches_wcag_reference_values() {
        assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio([0x76, 0x76, 0x76], [255, 255, 255]) - 4.54).abs() < 0.01);
    }
}
