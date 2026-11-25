use serde::{Deserialize, Serialize};

/// The largest scale reported. 2.0 is the 200% that WCAG 1.4.4 asks text to
/// reach; pinch zoom covers anything past it.
pub const MAX_SCALE: f64 = 2.0;

/// The smallest scale reported. Below the smallest setting either platform
/// offers (iOS xSmall is 14/17, about 0.82; Android's lowest is 0.85), so it
/// only guards against a nonsense value.
pub const MIN_SCALE: f64 = 0.5;

/// The phone's text settings, as the webview receives them.
///
/// `scale` is body text relative to the platform default: on iOS the Dynamic
/// Type body size divided by 17pt (the Large default), on Android
/// `Configuration.fontScale`. It is reported as is, so sizes below the default
/// come through under 1.0; the CSS that consumes it decides whether to floor.
/// `bold` is iOS Bold Text, or Android's font weight adjustment (API 31+).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSettings {
  pub scale: f64,
  pub bold: bool,
}

impl Default for TextSettings {
  fn default() -> Self {
    Self {
      scale: 1.0,
      bold: false,
    }
  }
}

impl TextSettings {
  /// Bounds the scale to what the web layer accepts. A value that is not a
  /// finite number reads as the default size.
  pub fn sanitized(self) -> Self {
    let scale = if self.scale.is_finite() {
      self.scale.clamp(MIN_SCALE, MAX_SCALE)
    } else {
      1.0
    };
    Self { scale, ..self }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keeps_a_scale_inside_the_bounds() {
    let xlarge = TextSettings {
      scale: 19.0 / 17.0,
      bold: true,
    };
    assert_eq!(xlarge.sanitized(), xlarge);
  }

  #[test]
  fn keeps_sizes_below_the_default() {
    let xsmall = TextSettings {
      scale: 14.0 / 17.0,
      bold: false,
    };
    assert_eq!(xsmall.sanitized().scale, 14.0 / 17.0);
  }

  #[test]
  fn caps_the_largest_accessibility_sizes() {
    let ax5 = TextSettings {
      scale: 53.0 / 17.0,
      bold: false,
    };
    assert_eq!(ax5.sanitized().scale, MAX_SCALE);
  }

  #[test]
  fn raises_a_nonsense_small_scale_to_the_floor() {
    let zero = TextSettings {
      scale: 0.0,
      bold: false,
    };
    assert_eq!(zero.sanitized().scale, MIN_SCALE);
  }

  #[test]
  fn reads_a_non_finite_scale_as_the_default() {
    for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
      let settings = TextSettings { scale, bold: true };
      assert_eq!(
        settings.sanitized(),
        TextSettings {
          scale: 1.0,
          bold: true
        }
      );
    }
  }

  #[test]
  fn parses_the_native_payload() {
    let parsed: TextSettings =
      serde_json::from_str(r#"{"scale":1.1176470588235294,"bold":true}"#).unwrap();
    assert_eq!(
      parsed,
      TextSettings {
        scale: 19.0 / 17.0,
        bold: true
      }
    );
  }

  #[test]
  fn serializes_in_the_shape_the_web_layer_reads() {
    let json = serde_json::to_value(TextSettings::default()).unwrap();
    assert_eq!(json, serde_json::json!({ "scale": 1.0, "bold": false }));
  }
}
