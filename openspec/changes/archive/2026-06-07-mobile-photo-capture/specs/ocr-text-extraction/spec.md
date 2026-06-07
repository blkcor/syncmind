## ADDED Requirements

### Requirement: Image captions are preserved in desktop markdown

Desktop `capture-image` dispatch SHALL parse the optional `caption` field from mobile image payloads and preserve non-empty captions in generated markdown as searchable body text. Caption text SHALL remain present after successful OCR post-processing, OCR no-text fallback, image decode failure fallback, and OCR initialization failure fallback.

#### Scenario: Captioned image writes caption into placeholder markdown
- **WHEN** a `capture-image` bundle is dispatched
- **AND** the payload includes a non-empty `caption`
- **THEN** desktop writes the image file to `<data-dir>/sync-inbox/images/<id>.jpg`
- **AND** writes `<data-dir>/sync-inbox/captures/<id>.md`
- **AND** the markdown body includes the caption text as plain markdown content
- **AND** the caption is indexed with the placeholder markdown

#### Scenario: Null caption does not create empty caption block
- **WHEN** a `capture-image` bundle is dispatched
- **AND** the payload has `caption = null` or an empty caption after trimming
- **THEN** desktop writes the same image and placeholder markdown paths
- **AND** the markdown does not include an empty caption section

#### Scenario: Successful OCR preserves caption
- **WHEN** a captioned `capture-image` bundle has already written placeholder markdown
- **AND** OCR later returns recognized text of at least 10 trimmed characters
- **THEN** desktop rewrites the markdown with the caption text still present
- **AND** includes the OCR text
- **AND** includes the `image_file` reference
- **AND** triggers re-index of the updated markdown

#### Scenario: OCR fallback preserves caption
- **WHEN** a captioned `capture-image` bundle has already written placeholder markdown
- **AND** OCR returns no usable text, fails to decode the image, or is unavailable
- **THEN** desktop keeps the caption text in markdown
- **AND** appends or preserves the appropriate fallback marker without deleting the caption
