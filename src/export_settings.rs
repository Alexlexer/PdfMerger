use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPreset {
    #[default]
    Lossless,
    Balanced,
    SmallerFile,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImagePagePolicy {
    #[default]
    A4Auto,
    OriginalAtDpi,
    Custom,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfMetadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub keywords: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportSettings {
    #[serde(default)]
    pub preset: ExportPreset,
    #[serde(default)]
    pub image_page_policy: ImagePagePolicy,
    #[serde(default = "default_margin")]
    pub margin_mm: f32,
    #[serde(default = "default_custom_width")]
    pub custom_width_mm: f32,
    #[serde(default = "default_custom_height")]
    pub custom_height_mm: f32,
    #[serde(default = "default_dpi")]
    pub original_dpi: f32,
    #[serde(default = "default_quality")]
    pub image_quality: u8,
    #[serde(default)]
    pub max_image_dimension: Option<u32>,
    #[serde(default)]
    pub metadata: PdfMetadata,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            preset: ExportPreset::Lossless,
            image_page_policy: ImagePagePolicy::A4Auto,
            margin_mm: default_margin(),
            custom_width_mm: default_custom_width(),
            custom_height_mm: default_custom_height(),
            original_dpi: default_dpi(),
            image_quality: default_quality(),
            max_image_dimension: None,
            metadata: PdfMetadata::default(),
        }
    }
}

impl ExportSettings {
    pub fn apply_preset(&mut self, preset: ExportPreset) {
        self.preset = preset;
        match preset {
            ExportPreset::Lossless => {
                self.image_quality = 100;
                self.max_image_dimension = None;
            }
            ExportPreset::Balanced => {
                self.image_quality = 85;
                self.max_image_dimension = Some(2400);
            }
            ExportPreset::SmallerFile => {
                self.image_quality = 65;
                self.max_image_dimension = Some(1600);
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !(0.0..=100.0).contains(&self.margin_mm) {
            bail!("image margin must be between 0 and 100 mm");
        }
        if !(1..=100).contains(&self.image_quality) {
            bail!("image quality must be between 1 and 100");
        }
        if let Some(maximum) = self.max_image_dimension
            && !(256..=20_000).contains(&maximum)
        {
            bail!("maximum image dimension must be between 256 and 20,000 pixels");
        }
        match self.image_page_policy {
            ImagePagePolicy::A4Auto => {
                if self.margin_mm * 2.0 >= 210.0 {
                    bail!("the image margin is too large for an A4 page");
                }
            }
            ImagePagePolicy::OriginalAtDpi => {
                if !(36.0..=1200.0).contains(&self.original_dpi) {
                    bail!("original-size DPI must be between 36 and 1,200");
                }
            }
            ImagePagePolicy::Custom => {
                if !(20.0..=2000.0).contains(&self.custom_width_mm)
                    || !(20.0..=2000.0).contains(&self.custom_height_mm)
                {
                    bail!("custom page dimensions must be between 20 and 2,000 mm");
                }
                if self.margin_mm * 2.0 >= self.custom_width_mm
                    || self.margin_mm * 2.0 >= self.custom_height_mm
                {
                    bail!("the image margin leaves no usable custom page area");
                }
            }
        }
        Ok(())
    }
}

const fn default_margin() -> f32 {
    10.0
}
const fn default_custom_width() -> f32 {
    210.0
}
const fn default_custom_height() -> f32 {
    297.0
}
const fn default_dpi() -> f32 {
    150.0
}
const fn default_quality() -> u8 {
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_apply_expected_quality_and_downsampling() {
        let mut settings = ExportSettings::default();
        settings.apply_preset(ExportPreset::Balanced);
        assert_eq!(settings.image_quality, 85);
        assert_eq!(settings.max_image_dimension, Some(2400));

        settings.apply_preset(ExportPreset::SmallerFile);
        assert_eq!(settings.image_quality, 65);
        assert_eq!(settings.max_image_dimension, Some(1600));

        settings.apply_preset(ExportPreset::Lossless);
        assert_eq!(settings.image_quality, 100);
        assert_eq!(settings.max_image_dimension, None);
    }

    #[test]
    fn rejects_page_layouts_without_usable_area() {
        let settings = ExportSettings {
            image_page_policy: ImagePagePolicy::Custom,
            custom_width_mm: 100.0,
            custom_height_mm: 100.0,
            margin_mm: 50.0,
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }
}
