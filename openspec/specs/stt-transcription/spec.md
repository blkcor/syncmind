# stt-transcription Specification Delta

## Purpose
Desktop-side speech-to-text transcription for mobile `capture-audio` bundles using whisper.cpp (`whisper-rs`).

## ADDED Requirements

### Requirement: Model auto-download on first use
The system SHALL download the selected Whisper model from the Hugging Face mirror on first STT invocation if not already present at `<data-dir>/models/whisper/<model-filename>.bin`.

#### Scenario: First-time download succeeds
- **WHEN** the first `capture-audio` bundle is dispatched
- **AND** no model file exists at `<data-dir>/models/whisper/ggml-base.en.bin`
- **AND** the HTTP download completes successfully
- **THEN** the system writes the model bytes to a `.tmp` file at the destination path
- **AND** the system computes SHA-256 of the downloaded bytes
- **AND** the system verifies the SHA-256 matches the expected hash for the model
- **AND** the system atomically renames `.tmp` → final filename
- **AND** the system proceeds to transcribe the audio

#### Scenario: Download fails gracefully
- **WHEN** the model download fails (network error / disk full / checksum mismatch)
- **THEN** the system logs the error to stderr
- **AND** the system marks the STT subsystem as `SttState::Unavailable`
- **AND** the system does NOT crash or block the ingest pipeline
- **AND** the audio binary file remains on disk unchanged
- **AND** the placeholder markdown body remains `"[mobile audio capture — transcription pending]"`

#### Scenario: Model directory creation
- **WHEN** the system needs to store a model file
- **AND** `<data-dir>/models/whisper/` does not exist
- **THEN** the system creates the directory with permission `0700`

### Requirement: Audio transcription
The system SHALL transcribe incoming `capture-audio` WAV content using the loaded Whisper model.

#### Scenario: Successful transcription
- **WHEN** a `capture-audio` bundle is dispatched
- **AND** the binary audio blob has been written to `<data-dir>/sync-inbox/audio/<id>.m4a`
- **AND** the placeholder markdown has been written to `<data-dir>/sync-inbox/captures/<id>.md`
- **AND** the STT subsystem is available (model loaded)
- **THEN** the system spawns a background task (`tokio::spawn_blocking`)
- **AND** the system decodes the `.m4a` to 16-bit 16kHz PCM via a simple FFmpeg or `hound` subprocess (or delegate to whisper.cpp's built-in WAV loading — require `<filename>.wav`)
- **AND** the system passes the PCM samples to `whisper_rs::WhisperContext::full()` with `translate=false`, `language=en`
- **AND** the system receives a Vec of `whisper_rs::Segment` with text and timestamps
- **AND** the system formats the output as SRT-like structure:
  ```text
  1
  00:00:00,000 --> 00:00:03,200
  Hello and welcome to my recording.

  2
  00:00:03,200 --> 00:00:06,500
  This is the next paragraph.
  ```
- **AND** the system atomically rewrites the `<id>.md` file:
  - frontmatter updated: `source: mobile-audio`, `stt_model: ggml-base.en`, `stt_engine: whisper`
  - body replaced with the SRT transcription
  - trailing block: `audio_file: ../audio/<id>.m4a`
- **AND** the system triggers a re-index of the updated `.md` file

#### Scenario: Model not loaded during dispatch
- **WHEN** a `capture-audio` bundle is dispatched
- **AND** the model download is still in progress
- **THEN** the system waits for download to complete (up to 120s timeout)
- **AND** proceeds with transcription on completion
- **AND** on timeout, the system defers transcription — leaves placeholder unchanged, retries on next bundle pull

### Requirement: STT failure degradation
The system SHALL handle STT failures gracefully without blocking the inbox pipeline.

#### Scenario: Transcription returns empty
- **WHEN** whisper inference completes with zero segments or all segments empty
- **THEN** the system appends `[transcription returned no text]` to the placeholder markdown body
- **AND** the system does NOT change the frontmatter `source` field

#### Scenario: Audio decode failure
- **WHEN** the `.m4a` binary cannot be decoded to PCM (corrupt / unsupported codec / empty)
- **THEN** the system logs the error
- **AND** the system appends `[audio decode failed — transcription unavailable]` to the body
- **AND** the system does NOT crash

### Requirement: STT lifecycle management
The system SHALL manage the Whisper context as a singleton shared via `Arc<Mutex<Option<WhisperContext>>>` so that model loading happens at most once.

#### Scenario: Whisper context is loaded once
- **WHEN** multiple `capture-audio` bundles arrive in quick succession
- **AND** the model is fully loaded after the first bundle
- **THEN** subsequent bundles share the same loaded context
- **AND** the `Arc<Mutex<...>>` guard is held only for the duration of `whisper_rs::WhisperContext::full()`
