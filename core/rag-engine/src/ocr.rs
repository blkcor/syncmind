use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use image::DynamicImage;
use image::ImageFormat;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;

static OCR_ENGINE: OnceLock<Result<OcrEngine, OcrError>> = OnceLock::new();

const DETECTION_MODEL_ENV: &str = "SYNCMIND_OCR_DETECTION_MODEL";
const RECOGNITION_MODEL_ENV: &str = "SYNCMIND_OCR_RECOGNITION_MODEL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrError {
    Init(String),
    Decode(String),
    Recognition(String),
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Init(message) => write!(f, "OCR initialization failed: {message}"),
            Self::Decode(message) => write!(f, "image decode failed: {message}"),
            Self::Recognition(message) => write!(f, "OCR recognition failed: {message}"),
        }
    }
}

impl std::error::Error for OcrError {}

pub fn ocr_image<P: AsRef<Path>>(_path: P) -> Result<String, OcrError> {
    let image = image::open(_path.as_ref()).map_err(|error| OcrError::Decode(error.to_string()))?;
    recognize_dynamic_image(image)
}

pub fn ocr_image_from_bytes(_bytes: &[u8], _format: ImageFormat) -> Result<String, OcrError> {
    let image = image::load_from_memory_with_format(_bytes, _format)
        .map_err(|error| OcrError::Decode(error.to_string()))?;
    recognize_dynamic_image(image)
}

fn recognize_dynamic_image(image: DynamicImage) -> Result<String, OcrError> {
    let image = image.into_rgb8();
    let source = ImageSource::from_bytes(image.as_raw(), image.dimensions())
        .map_err(|error| OcrError::Decode(error.to_string()))?;
    let engine = engine()?;
    let input = engine
        .prepare_input(source)
        .map_err(|error| OcrError::Recognition(error.to_string()))?;
    engine
        .get_text(&input)
        .map_err(|error| OcrError::Recognition(error.to_string()))
}

fn engine() -> Result<&'static OcrEngine, OcrError> {
    match OCR_ENGINE.get_or_init(load_engine) {
        Ok(engine) => Ok(engine),
        Err(error) => Err(error.clone()),
    }
}

fn load_engine() -> Result<OcrEngine, OcrError> {
    let detection_model_path = std::env::var(DETECTION_MODEL_ENV).map_err(|_| {
        OcrError::Init(format!(
            "{DETECTION_MODEL_ENV} must point to text-detection.rten"
        ))
    })?;
    let recognition_model_path = std::env::var(RECOGNITION_MODEL_ENV).map_err(|_| {
        OcrError::Init(format!(
            "{RECOGNITION_MODEL_ENV} must point to text-recognition.rten"
        ))
    })?;

    let detection_model = Model::load_file(&detection_model_path).map_err(|error| {
        OcrError::Init(format!(
            "failed to load detection model at {detection_model_path}: {error}"
        ))
    })?;
    let recognition_model = Model::load_file(&recognition_model_path).map_err(|error| {
        OcrError::Init(format!(
            "failed to load recognition model at {recognition_model_path}: {error}"
        ))
    })?;

    OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .map_err(|error| OcrError::Init(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_bytes_return_decode_error() {
        let result = ocr_image_from_bytes(b"not a real png", ImageFormat::Png);

        assert!(
            matches!(result, Err(OcrError::Decode(_))),
            "expected decode error for corrupt image bytes, got {result:?}"
        );
    }

    #[test]
    fn degenerate_image_without_models_returns_init_error() {
        if std::env::var(DETECTION_MODEL_ENV).is_ok() || std::env::var(RECOGNITION_MODEL_ENV).is_ok()
        {
            return;
        }

        let image = image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let result = ocr_image_from_bytes(&bytes, ImageFormat::Png);

        assert!(
            matches!(result, Err(OcrError::Init(_))),
            "expected OCR init error when model env vars are absent, got {result:?}"
        );
    }
}
