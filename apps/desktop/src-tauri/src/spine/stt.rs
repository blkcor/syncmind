use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use whisper_rs::{
    convert_integer_to_float_audio, convert_stereo_to_mono_audio, FullParams, SamplingStrategy,
    WhisperContext, WhisperContextParameters,
};

const MODEL_NAME: &str = "ggml-base.en";
const MODEL_FILENAME: &str = "ggml-base.en.bin";
const MODEL_SHA256: &str = "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002";
const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

static STT_STATE: OnceLock<Mutex<SttState>> = OnceLock::new();

#[derive(Debug)]
pub enum SttState {
    Unavailable,
    Downloading,
    Ready(Arc<Mutex<WhisperContext>>),
}

#[derive(Debug)]
pub enum SttError {
    Io(std::io::Error),
    Download(String),
    Decode(String),
    Transcription(String),
}

impl fmt::Display for SttError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "STT IO error: {error}"),
            Self::Download(message) => write!(f, "STT model download failed: {message}"),
            Self::Decode(message) => write!(f, "audio decode failed: {message}"),
            Self::Transcription(message) => write!(f, "audio transcription failed: {message}"),
        }
    }
}

impl std::error::Error for SttError {}

impl From<std::io::Error> for SttError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptSegment {
    start_cs: i64,
    end_cs: i64,
    text: String,
}

pub async fn ensure_model(data_dir: &Path) -> Result<PathBuf, SttError> {
    let model_path = model_path(data_dir);
    if model_path.exists() {
        verify_model_hash(&model_path)?;
        return Ok(model_path);
    }

    download_model(data_dir).await
}

pub async fn transcribe_audio(
    audio_path: PathBuf,
    markdown_path: PathBuf,
    data_dir: PathBuf,
) -> Result<bool, SttError> {
    let result = run_transcription(audio_path.clone(), data_dir).await;
    match result {
        Ok(Some(transcription)) => {
            rewrite_markdown_with_transcription(&markdown_path, &audio_path, &transcription)
                .await?;
            Ok(true)
        }
        Ok(None) => {
            append_marker(&markdown_path, "[transcription returned no text]").await?;
            Ok(false)
        }
        Err(SttError::Decode(message)) => {
            tracing::warn!(path = %audio_path.display(), error = %message, "audio decode failed");
            append_marker(
                &markdown_path,
                "[audio decode failed - transcription unavailable]",
            )
            .await?;
            Ok(false)
        }
        Err(error) => {
            tracing::warn!(path = %audio_path.display(), error = %error, "audio transcription unavailable");
            Err(error)
        }
    }
}

async fn run_transcription(
    audio_path: PathBuf,
    data_dir: PathBuf,
) -> Result<Option<String>, SttError> {
    let model_path = ensure_model(&data_dir).await.inspect_err(|_| {
        if let Some(state) = STT_STATE.get() {
            if let Ok(mut guard) = state.try_lock() {
                *guard = SttState::Unavailable;
            }
        }
    })?;

    let pcm = tokio::task::spawn_blocking(move || decode_audio_to_pcm(&audio_path))
        .await
        .map_err(|error| SttError::Decode(error.to_string()))??;
    let segments = tokio::task::spawn_blocking(move || transcribe_pcm(&model_path, &pcm))
        .await
        .map_err(|error| SttError::Transcription(error.to_string()))??;

    Ok(format_transcription(&segments))
}

async fn download_model(data_dir: &Path) -> Result<PathBuf, SttError> {
    let model_path = model_path(data_dir);
    let model_dir = model_path
        .parent()
        .ok_or_else(|| SttError::Download("model path has no parent".to_string()))?;
    fs::create_dir_all(model_dir)?;
    set_dir_permissions_0700(model_dir)?;

    let tmp_path = model_path.with_extension("bin.tmp");
    {
        let mut state = stt_state().lock().await;
        *state = SttState::Downloading;
    }

    let response = reqwest::get(MODEL_URL)
        .await
        .map_err(|error| SttError::Download(error.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        mark_unavailable().await;
        return Err(SttError::Download(format!(
            "model download returned {status}"
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| SttError::Download(error.to_string()))?;

    let downloaded_hash = hex::encode(Sha256::digest(&bytes));
    if downloaded_hash != MODEL_SHA256 {
        mark_unavailable().await;
        return Err(SttError::Download(format!(
            "model checksum mismatch: expected {MODEL_SHA256}, got {downloaded_hash}"
        )));
    }

    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, &model_path)?;
    Ok(model_path)
}

fn model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("whisper").join(MODEL_FILENAME)
}

fn verify_model_hash(path: &Path) -> Result<(), SttError> {
    let bytes = fs::read(path)?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual == MODEL_SHA256 {
        Ok(())
    } else {
        Err(SttError::Download(format!(
            "model checksum mismatch: expected {MODEL_SHA256}, got {actual}"
        )))
    }
}

fn stt_state() -> &'static Mutex<SttState> {
    STT_STATE.get_or_init(|| Mutex::new(SttState::Unavailable))
}

async fn mark_unavailable() {
    let mut state = stt_state().lock().await;
    *state = SttState::Unavailable;
}

fn load_whisper_context(model_path: &Path) -> Result<WhisperContext, SttError> {
    WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|error| SttError::Transcription(error.to_string()))
}

fn transcribe_pcm(model_path: &Path, pcm: &[f32]) -> Result<Vec<TranscriptSegment>, SttError> {
    let context = {
        let mut state = stt_state().blocking_lock();
        match &*state {
            SttState::Ready(context) => context.clone(),
            SttState::Downloading | SttState::Unavailable => {
                let context = Arc::new(Mutex::new(load_whisper_context(model_path)?));
                *state = SttState::Ready(context.clone());
                context
            }
        }
    };

    let context = context.blocking_lock();
    let mut state = context
        .create_state()
        .map_err(|error| SttError::Transcription(error.to_string()))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_translate(false);
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, pcm)
        .map_err(|error| SttError::Transcription(error.to_string()))?;

    let segments = state
        .as_iter()
        .filter_map(|segment| {
            let text = segment.to_string().trim().to_string();
            (!text.is_empty()).then(|| TranscriptSegment {
                start_cs: segment.start_timestamp(),
                end_cs: segment.end_timestamp(),
                text,
            })
        })
        .collect();
    Ok(segments)
}

fn decode_audio_to_pcm(audio_path: &Path) -> Result<Vec<f32>, SttError> {
    let tmp_wav = audio_path.with_extension("syncmind-stt.wav");
    let output = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(audio_path)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-sample_fmt")
        .arg("s16")
        .arg(&tmp_wav)
        .output()
        .map_err(|error| SttError::Decode(format!("failed to run ffmpeg: {error}")))?;
    if !output.status.success() {
        return Err(SttError::Decode(format!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let reader =
        hound::WavReader::open(&tmp_wav).map_err(|error| SttError::Decode(error.to_string()))?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 {
        return Err(SttError::Decode(format!(
            "expected 16kHz WAV after ffmpeg, got {}Hz",
            spec.sample_rate
        )));
    }
    let channels = spec.channels;
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SttError::Decode(error.to_string()))?;
    let mut audio = vec![0.0f32; samples.len()];
    convert_integer_to_float_audio(&samples, &mut audio)
        .map_err(|error| SttError::Decode(error.to_string()))?;
    let _ = fs::remove_file(&tmp_wav);

    if channels == 1 {
        Ok(audio)
    } else if channels == 2 {
        let mut mono = vec![0.0f32; audio.len() / 2];
        convert_stereo_to_mono_audio(&audio, &mut mono)
            .map_err(|error| SttError::Decode(error.to_string()))?;
        Ok(mono)
    } else {
        Err(SttError::Decode(format!(
            "unsupported channel count: {channels}"
        )))
    }
}

fn format_transcription(segments: &[TranscriptSegment]) -> Option<String> {
    let blocks = segments
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .enumerate()
        .map(|(idx, segment)| {
            format!(
                "{}\n{} --> {}\n{}",
                idx + 1,
                format_timestamp(segment.start_cs),
                format_timestamp(segment.end_cs),
                segment.text.trim()
            )
        })
        .collect::<Vec<_>>();

    (!blocks.is_empty()).then(|| blocks.join("\n\n"))
}

fn format_timestamp(centiseconds: i64) -> String {
    let millis = centiseconds.max(0) * 10;
    let hours = millis / 3_600_000;
    let minutes = (millis % 3_600_000) / 60_000;
    let seconds = (millis % 60_000) / 1_000;
    let millis = millis % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

async fn rewrite_markdown_with_transcription(
    markdown_path: &Path,
    audio_path: &Path,
    transcription: &str,
) -> Result<(), SttError> {
    let audio_file = audio_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(MODEL_FILENAME);
    let content = format!(
        "---\nsource: mobile-audio\nstt_model: {MODEL_NAME}\nstt_engine: whisper\n---\n\n{transcription}\n\n---\naudio_file: ../audio/{audio_file}\n"
    );
    write_text_atomically(markdown_path, &content).await
}

async fn append_marker(path: &Path, marker: &str) -> Result<(), SttError> {
    let mut content = tokio::fs::read_to_string(path).await?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(marker);
    content.push('\n');
    write_text_atomically(path, &content).await
}

async fn write_text_atomically(path: &Path, content: &str) -> Result<(), SttError> {
    let tmp_path = tmp_path(path);
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

#[cfg(unix)]
fn set_dir_permissions_0700(path: &Path) -> Result<(), SttError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_permissions_0700(_path: &Path) -> Result<(), SttError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_centisecond_timestamps_as_srt_time() {
        assert_eq!(format_timestamp(0), "00:00:00,000");
        assert_eq!(format_timestamp(320), "00:00:03,200");
        assert_eq!(format_timestamp(366_100), "01:01:01,000");
    }

    #[test]
    fn empty_segments_are_detected_before_markdown_rewrite() {
        let formatted = format_transcription(&[]);
        assert!(formatted.is_none());
    }

    #[test]
    fn formats_segments_as_srt_blocks() {
        let formatted = format_transcription(&[
            TranscriptSegment {
                start_cs: 0,
                end_cs: 320,
                text: "Hello and welcome.".to_string(),
            },
            TranscriptSegment {
                start_cs: 320,
                end_cs: 650,
                text: "This is the next paragraph.".to_string(),
            },
        ])
        .unwrap();

        assert!(formatted.contains("1\n00:00:00,000 --> 00:00:03,200\nHello and welcome."));
        assert!(formatted.contains("2\n00:00:03,200 --> 00:00:06,500\nThis is the next paragraph."));
    }

    #[tokio::test]
    async fn append_marker_updates_placeholder_without_frontmatter_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture.md");
        let original = "---\nsource: mobile-capture\nkind: capture-audio\n---\n\n[pending]\n";
        std::fs::write(&path, original).unwrap();

        append_marker(&path, "[audio decode failed - transcription unavailable]")
            .await
            .unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.starts_with("---\nsource: mobile-capture\nkind: capture-audio\n---"));
        assert!(updated.contains("[pending]"));
        assert!(updated.contains("[audio decode failed - transcription unavailable]"));
    }

    #[tokio::test]
    async fn ensure_model_rejects_existing_file_with_bad_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let model = model_path(dir.path());
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"not the expected model").unwrap();

        let result = ensure_model(dir.path()).await;

        assert!(
            matches!(result, Err(SttError::Download(message)) if message.contains("checksum mismatch"))
        );
    }

    #[test]
    fn invalid_model_file_returns_transcription_error() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("bad.bin");
        std::fs::write(&model, b"not a whisper model").unwrap();

        let result = load_whisper_context(&model);

        assert!(matches!(result, Err(SttError::Transcription(_))));
    }
}
