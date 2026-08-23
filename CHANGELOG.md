# Changelog

## [Unreleased]

## [0.1.0] - 2026-08-23

Initial public release.

### Added

- Add parser-aware removal of artificial Markdown soft-line breaks while preserving syntax-significant structure.
- Add optional Unicode-display-width reflow with protected Markdown spans and idempotent output.
- Add standard-input, standard-output, explicit-file, and recursive-directory modes.
- Add check and diff modes, configuration discovery, include patterns, ignore handling, deterministic parallel processing, and atomic per-file replacement.

### Safety

- Preserve a leading UTF-8 byte-order mark while parsing and formatting Markdown.
- Refuse custom `:::` containers outside recognized literal blocks and filename-bearing `.mdx` input in filesystem modes.
- Preserve LF, CRLF, and bare-carriage-return line endings while unwrapping and width-reflowing Markdown.

### Changed

- Change `format_markdown` and `unwrap_markdown` to return `Result<String, FormatError>` and report unsupported input explicitly.

[Unreleased]: https://github.com/flyingrobots/wide-md/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/flyingrobots/wide-md/releases/tag/v0.1.0
