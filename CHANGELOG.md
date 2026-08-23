# Changelog

## Unreleased

- Change `format_markdown` and `unwrap_markdown` to return `Result<String, FormatError>` and report unsupported input explicitly.
- Preserve a leading UTF-8 byte-order mark while parsing and formatting Markdown.
- Refuse custom `:::` containers outside recognized literal blocks and filename-bearing `.mdx` input in filesystem modes.
- Preserve bare-carriage-return line endings while unwrapping and width-reflowing Markdown.
