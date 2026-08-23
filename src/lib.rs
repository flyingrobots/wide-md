//! Markdown-aware removal of soft line breaks.

pub mod cli;

use std::num::NonZeroUsize;
use std::ops::Range;
use std::{error, fmt};

use pulldown_cmark::{Event, Options, Parser, Tag};
use unicode_width::UnicodeWidthChar;

/// Formatting controls for [`format_markdown`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FormatOptions {
    /// Maximum source width in Unicode display columns. When absent, prose is
    /// fully unwrapped to one physical line per Markdown paragraph.
    pub width: Option<NonZeroUsize>,
}

/// A source construct that cannot be formatted safely by the built-in
/// Markdown profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatError {
    /// A `:::` custom container marker outside a literal block.
    UnsupportedCustomContainer { line: usize },
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomContainer { line } => write!(
                formatter,
                "unsupported custom container syntax at line {line}; `:::` containers require a dialect-aware formatter"
            ),
        }
    }
}

impl error::Error for FormatError {}

/// Formats Markdown by removing soft wrapping and, when requested, reflowing
/// prose to a maximum source width. A leading UTF-8 byte-order mark is
/// preserved around the complete formatting pipeline.
///
/// # Errors
///
/// Returns [`FormatError`] when the source contains a known unsupported
/// construct that the built-in Markdown profile cannot rewrite safely.
pub fn format_markdown(input: &str, options: FormatOptions) -> Result<String, FormatError> {
    let (body, has_bom) = split_bom(input);
    reject_unsupported_syntax(body)?;

    let unwrapped = unwrap_markdown_body(body);
    let Some(width) = options.width else {
        return Ok(restore_bom(unwrapped, has_bom));
    };

    Ok(restore_bom(
        reflow_markdown(&unwrapped, width.get()),
        has_bom,
    ))
}

/// Removes soft line breaks from Markdown prose while leaving every other
/// source byte untouched.
///
/// Markdown's parser decides which newlines are soft breaks. Newlines that
/// define block structure, fenced or indented code, tables, metadata, raw HTML,
/// or explicit hard breaks are therefore preserved without bespoke rewriting.
/// A leading UTF-8 byte-order mark is preserved and excluded from parsing.
///
/// # Errors
///
/// Returns [`FormatError`] when the source contains a known unsupported
/// construct that the built-in Markdown profile cannot rewrite safely.
pub fn unwrap_markdown(input: &str) -> Result<String, FormatError> {
    let (body, has_bom) = split_bom(input);
    reject_unsupported_syntax(body)?;
    Ok(restore_bom(unwrap_markdown_body(body), has_bom))
}

fn unwrap_markdown_body(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut copied_through = 0;

    for (event, source) in Parser::new_ext(input, parser_options()).into_offset_iter() {
        if matches!(event, Event::SoftBreak) && !follows_gfm_alert_marker(input, source.start) {
            debug_assert!(source.start >= copied_through);
            output.push_str(&input[copied_through..source.start]);
            while matches!(output.as_bytes().last(), Some(b' ' | b'\t')) {
                output.pop();
            }
            output.push(' ');
            copied_through = continuation_content_start(input, source.end);
        }
    }

    if copied_through == 0 {
        return input.to_owned();
    }

    output.push_str(&input[copied_through..]);
    output
}

fn split_bom(input: &str) -> (&str, bool) {
    input
        .strip_prefix('\u{feff}')
        .map_or((input, false), |body| (body, true))
}

fn restore_bom(output: String, has_bom: bool) -> String {
    if !has_bom {
        return output;
    }

    let mut with_bom = String::with_capacity('\u{feff}'.len_utf8() + output.len());
    with_bom.push('\u{feff}');
    with_bom.push_str(&output);
    with_bom
}

fn reject_unsupported_syntax(input: &str) -> Result<(), FormatError> {
    if let Some(line) = custom_container_line(input) {
        return Err(FormatError::UnsupportedCustomContainer { line });
    }
    Ok(())
}

fn custom_container_line(input: &str) -> Option<usize> {
    let literal_ranges = literal_block_ranges(input);
    let mut line_start = 0;

    for (line_index, source_line) in input.split_inclusive('\n').enumerate() {
        let line_end = line_start + source_line.len();
        let is_literal = literal_ranges
            .iter()
            .any(|range| range.start < line_end && range.end > line_start);
        let line = source_line.strip_suffix('\n').unwrap_or(source_line);
        let line = line.strip_suffix('\r').unwrap_or(line);

        if !is_literal && line_starts_custom_container(line) {
            return Some(line_index + 1);
        }
        line_start = line_end;
    }

    None
}

fn literal_block_ranges(input: &str) -> Vec<Range<usize>> {
    Parser::new_ext(input, parser_options())
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::MetadataBlock(_)) => Some(range),
            _ => None,
        })
        .collect()
}

fn line_starts_custom_container(line: &str) -> bool {
    wrapping_parts(line).is_some_and(|parts| {
        parts
            .content
            .trim_start_matches([' ', '\t'])
            .starts_with(":::")
    })
}

fn reflow_markdown(input: &str, width: usize) -> String {
    let protected = protected_ranges(input);
    let inserted_newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let mut output = String::with_capacity(input.len());
    let mut line_start = 0;

    while line_start < input.len() {
        let (line_end, body_end, line_ending) = match input[line_start..].find('\n') {
            Some(relative_end) => {
                let line_end = line_start + relative_end + 1;
                let lf = line_end - 1;
                if lf > line_start && input.as_bytes()[lf - 1] == b'\r' {
                    (line_end, lf - 1, "\r\n")
                } else {
                    (line_end, lf, "\n")
                }
            }
            None => (input.len(), input.len(), ""),
        };

        let body = &input[line_start..body_end];
        let is_protected = protected
            .iter()
            .any(|range| range.start < line_end && range.end > line_start);

        if is_protected || display_width(body) <= width {
            output.push_str(&input[line_start..line_end]);
        } else {
            output.push_str(&wrap_source_line(
                body,
                line_ending,
                inserted_newline,
                width,
            ));
        }

        line_start = line_end;
    }

    output
}

fn protected_ranges(input: &str) -> Vec<Range<usize>> {
    let parser = Parser::new_ext(input, parser_options());
    let mut ranges: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| definition.span.clone())
        .collect();

    for (event, range) in parser.into_offset_iter() {
        let protect = match event {
            Event::Start(tag) => matches!(
                tag,
                Tag::Heading { .. }
                    | Tag::CodeBlock(_)
                    | Tag::HtmlBlock
                    | Tag::Table(_)
                    | Tag::MetadataBlock(_)
            ),
            Event::Rule | Event::InlineMath(_) | Event::DisplayMath(_) => true,
            _ => false,
        };
        if protect {
            ranges.push(range);
        }
    }

    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn wrap_source_line(body: &str, line_ending: &str, inserted_newline: &str, width: usize) -> String {
    let trailing_spaces = body.bytes().rev().take_while(|byte| *byte == b' ').count();
    let (wrappable, hard_break_suffix) = if trailing_spaces >= 2 {
        body.split_at(body.len() - trailing_spaces)
    } else {
        (body, "")
    };

    let Some(parts) = wrapping_parts(wrappable) else {
        return format!("{body}{line_ending}");
    };
    if is_structural_prose_line(parts.content) {
        return format!("{body}{line_ending}");
    }

    let words = markdown_words(parts.content);
    if words.len() < 2 {
        return format!("{body}{line_ending}");
    }

    let mut lines = Vec::new();
    let mut current = String::from(parts.first_prefix);
    let mut has_word = false;

    for word in words {
        let separator_width = usize::from(has_word);
        let candidate_width = display_width(&current) + separator_width + display_width(word);
        if has_word && candidate_width > width {
            lines.push(current);
            current = parts.continuation_prefix.clone();
            current.push_str(word);
        } else {
            if has_word {
                current.push(' ');
            }
            current.push_str(word);
        }
        has_word = true;
    }
    lines.push(current);

    if lines.len() == 1 {
        return format!("{body}{line_ending}");
    }

    let break_sequence = if line_ending.is_empty() {
        inserted_newline
    } else {
        line_ending
    };
    let mut rendered = lines.join(break_sequence);
    rendered.push_str(hard_break_suffix);
    rendered.push_str(line_ending);
    rendered
}

#[derive(Debug)]
struct WrappingParts<'a> {
    first_prefix: &'a str,
    continuation_prefix: String,
    content: &'a str,
}

fn wrapping_parts(line: &str) -> Option<WrappingParts<'_>> {
    let bytes = line.as_bytes();
    let quote_end = quote_prefix_end(line);
    let mut marker_start = quote_end;
    while matches!(bytes.get(marker_start), Some(b' ' | b'\t')) {
        marker_start += 1;
    }

    if let Some(prefix_end) = list_prefix_end(line, marker_start) {
        let content_indent = display_width(&line[quote_end..prefix_end]);
        let mut first_prefix_end = prefix_end;
        if let Some(task_end) = task_marker_end(line, prefix_end) {
            first_prefix_end = task_end;
        }
        return Some(WrappingParts {
            first_prefix: &line[..first_prefix_end],
            continuation_prefix: format!("{}{}", &line[..quote_end], " ".repeat(content_indent)),
            content: &line[first_prefix_end..],
        });
    }

    if let Some(prefix_end) = footnote_prefix_end(line, marker_start) {
        let content_indent = display_width(&line[quote_end..prefix_end]).max(4);
        return Some(WrappingParts {
            first_prefix: &line[..prefix_end],
            continuation_prefix: format!("{}{}", &line[..quote_end], " ".repeat(content_indent)),
            content: &line[prefix_end..],
        });
    }

    if bytes.get(marker_start) == Some(&b':')
        && matches!(bytes.get(marker_start + 1), Some(b' ' | b'\t'))
    {
        let prefix_end = consume_ascii_whitespace(line, marker_start + 1);
        let content_indent = display_width(&line[quote_end..prefix_end]);
        return Some(WrappingParts {
            first_prefix: &line[..prefix_end],
            continuation_prefix: format!("{}{}", &line[..quote_end], " ".repeat(content_indent)),
            content: &line[prefix_end..],
        });
    }

    Some(WrappingParts {
        first_prefix: &line[..marker_start],
        continuation_prefix: line[..marker_start].to_owned(),
        content: &line[marker_start..],
    })
}

fn quote_prefix_end(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut cursor = 0;

    loop {
        let checkpoint = cursor;
        let mut spaces = 0;
        while bytes.get(cursor) == Some(&b' ') && spaces < 3 {
            cursor += 1;
            spaces += 1;
        }
        if bytes.get(cursor) != Some(&b'>') {
            cursor = checkpoint;
            break;
        }
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
    }

    cursor
}

fn list_prefix_end(line: &str, marker_start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let marker_end = if matches!(bytes.get(marker_start), Some(b'-' | b'+' | b'*')) {
        marker_start + 1
    } else {
        let digits = bytes[marker_start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if !(1..=9).contains(&digits)
            || !matches!(bytes.get(marker_start + digits), Some(b'.' | b')'))
        {
            return None;
        }
        marker_start + digits + 1
    };

    matches!(bytes.get(marker_end), Some(b' ' | b'\t'))
        .then(|| consume_ascii_whitespace(line, marker_end))
}

fn task_marker_end(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.get(start) != Some(&b'[')
        || !matches!(bytes.get(start + 1), Some(b' ' | b'x' | b'X'))
        || bytes.get(start + 2) != Some(&b']')
        || !matches!(bytes.get(start + 3), Some(b' ' | b'\t'))
    {
        return None;
    }
    Some(consume_ascii_whitespace(line, start + 3))
}

fn footnote_prefix_end(line: &str, start: usize) -> Option<usize> {
    let rest = line.get(start..)?;
    if !rest.starts_with("[^") {
        return None;
    }
    let closing = rest.find("]:")?;
    let marker_end = start + closing + 2;
    matches!(line.as_bytes().get(marker_end), Some(b' ' | b'\t'))
        .then(|| consume_ascii_whitespace(line, marker_end))
}

fn consume_ascii_whitespace(line: &str, mut cursor: usize) -> usize {
    while matches!(line.as_bytes().get(cursor), Some(b' ' | b'\t')) {
        cursor += 1;
    }
    cursor
}

fn is_structural_prose_line(content: &str) -> bool {
    let content = content.trim();
    content.is_empty()
        || matches!(
            content,
            "[!NOTE]" | "[!TIP]" | "[!IMPORTANT]" | "[!WARNING]" | "[!CAUTION]"
        )
        || content.starts_with(":::")
        || content.starts_with("import ")
        || content.starts_with("export ")
}

fn markdown_words(input: &str) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut words = Vec::new();
    let mut cursor = 0;

    while cursor < bytes.len() {
        while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }

        let word_start = cursor;
        while cursor < bytes.len() && !matches!(bytes[cursor], b' ' | b'\t') {
            if bytes[cursor] == b'`'
                && let Some(span_end) = code_span_end(input, cursor)
            {
                cursor = span_end;
            } else if bytes[cursor] == b'<'
                && let Some(relative_end) = input[cursor..].find('>')
            {
                cursor += relative_end + 1;
            } else if matches!(bytes[cursor], b'[' | b'!')
                && let Some(span_end) = inline_link_end(input, cursor)
            {
                cursor = span_end;
            } else {
                cursor += input[cursor..]
                    .chars()
                    .next()
                    .expect("cursor is inside the string")
                    .len_utf8();
            }
        }
        words.push(&input[word_start..cursor]);
    }

    words
}

fn code_span_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let delimiter_length = bytes[start..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count();
    let mut cursor = start + delimiter_length;

    while cursor < bytes.len() {
        if bytes[cursor] == b'`' {
            let run = bytes[cursor..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if run == delimiter_length {
                return Some(cursor + run);
            }
            cursor += run;
        } else {
            cursor += input[cursor..].chars().next()?.len_utf8();
        }
    }
    None
}

fn inline_link_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let bracket_start = if bytes.get(start) == Some(&b'!') {
        start + 1
    } else {
        start
    };
    if bytes.get(bracket_start) != Some(&b'[') {
        return None;
    }

    let label_end = input[bracket_start + 1..].find("](")? + bracket_start + 1;
    let mut depth = 1;
    let mut cursor = label_end + 2;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = (cursor + 2).min(bytes.len()),
            b'(' => {
                depth += 1;
                cursor += 1;
            }
            b')' => {
                depth -= 1;
                cursor += 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {
                cursor += input[cursor..].chars().next()?.len_utf8();
            }
        }
    }
    None
}

fn display_width(input: &str) -> usize {
    input
        .chars()
        .map(|character| {
            if character == '\t' {
                4
            } else {
                UnicodeWidthChar::width(character).unwrap_or(0)
            }
        })
        .sum()
}

fn continuation_content_start(input: &str, mut cursor: usize) -> usize {
    let bytes = input.as_bytes();

    loop {
        while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }

        if bytes.get(cursor) != Some(&b'>') {
            break;
        }

        cursor += 1;
        if matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
    }

    cursor
}

fn follows_gfm_alert_marker(input: &str, soft_break_start: usize) -> bool {
    let line_start = input[..soft_break_start]
        .rfind(['\n', '\r'])
        .map_or(0, |index| index + 1);
    let mut content = input[line_start..soft_break_start].trim();

    while let Some(after_marker) = content.strip_prefix('>') {
        content = after_marker.trim_start();
    }

    matches!(
        content,
        "[!NOTE]" | "[!TIP]" | "[!IMPORTANT]" | "[!WARNING]" | "[!CAUTION]"
    )
}

fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_WIKILINKS
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::{FormatError, FormatOptions, format_markdown, unwrap_markdown};

    fn unwrap(input: &str) -> String {
        unwrap_markdown(input).expect("test input should use supported Markdown syntax")
    }

    fn format_at_width(input: &str, width: usize) -> String {
        format_markdown(
            input,
            FormatOptions {
                width: NonZeroUsize::new(width),
            },
        )
        .expect("test input should use supported Markdown syntax")
    }

    #[test]
    fn joins_soft_wrapped_paragraphs() {
        let input = "This paragraph was wrapped at\neighty columns and should become\none physical line.\n\nAnother paragraph.\n";
        let expected = "This paragraph was wrapped at eighty columns and should become one physical line.\n\nAnother paragraph.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn preserves_headings_rules_and_unwraps_setext_headings() {
        let input = "# Heading\n\nA wrapped setext\nheading\n=======\n\n---\n\nText\n";
        let expected = "# Heading\n\nA wrapped setext heading\n=======\n\n---\n\nText\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn preserves_fenced_and_indented_code() {
        let input = "Before this\ncode.\n\n```rust\nlet value =\n    42;\n```\n\n    indented();\n    still_indented();\n";
        let expected = "Before this code.\n\n```rust\nlet value =\n    42;\n```\n\n    indented();\n    still_indented();\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn unwraps_nested_list_items_without_merging_items() {
        let input = "- A list item that was\n  wrapped across lines.\n- A sibling item.\n    - A nested item that is\n      wrapped too.\n\n1. An ordered item that\n   also wraps.\n2. Another item.\n";
        let expected = "- A list item that was wrapped across lines.\n- A sibling item.\n    - A nested item that is wrapped too.\n\n1. An ordered item that also wraps.\n2. Another item.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn unwraps_matching_blockquotes_and_gfm_alerts() {
        let input = "> A quoted paragraph that\n> was wrapped.\n>\n> [!NOTE]\n> An alert paragraph that\n> was wrapped.\n";
        let expected = "> A quoted paragraph that was wrapped.\n>\n> [!NOTE]\n> An alert paragraph that was wrapped.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn preserves_explicit_hard_breaks() {
        let input = "First line.  \nSecond line that\ncan still unwrap.\n\nBackslash break.\\\nLast line.\n";
        let expected =
            "First line.  \nSecond line that can still unwrap.\n\nBackslash break.\\\nLast line.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn preserves_gfm_tables() {
        let input = "| Name | Meaning |\n| --- | --- |\n| wide-md | Unwraps text |\n\nA wrapped paragraph\nafter the table.\n";
        let expected = "| Name | Meaning |\n| --- | --- |\n| wide-md | Unwraps text |\n\nA wrapped paragraph after the table.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn preserves_metadata_raw_html_and_display_math() {
        let input = "---\ntitle: A document\ndescription: Kept\nas written\n---\n\n<div>\nraw lines\nstay separate\n</div>\n\n$$\na +\nb\n$$\n\nWrapped\nprose.\n";
        let expected = "---\ntitle: A document\ndescription: Kept\nas written\n---\n\n<div>\nraw lines\nstay separate\n</div>\n\n$$\na +\nb\n$$\n\nWrapped prose.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn preserves_utf8_bom_around_default_and_width_formatting() {
        let input = "\u{feff}---\ntitle: A document\ndescription: Kept\nas written\n---\n\nWrapped\nprose.\n";
        let expected = "\u{feff}---\ntitle: A document\ndescription: Kept\nas written\n---\n\nWrapped prose.\n";

        assert_eq!(unwrap(input), expected);
        assert_eq!(unwrap("\u{feff}"), "\u{feff}");

        let width_input =
            "\u{feff}+++\r\ntitle = \"Test\"\r\n+++\r\n\r\nOne two three four five six.\r\n";
        let width_expected =
            "\u{feff}+++\r\ntitle = \"Test\"\r\n+++\r\n\r\nOne two three\r\nfour five six.\r\n";
        assert_eq!(format_at_width(width_input, 14), width_expected);
    }

    #[test]
    fn refuses_custom_containers_in_prose_containers() {
        let fixtures = [
            ("Before.\n\n:::note\nBody.\n:::\n", 3),
            ("Before.\n\n> - :::note\n>   Body.\n>   :::\n", 3),
            ("Term\n: :::note\n  Body.\n  :::\n", 2),
            ("[^note]: :::note\n    Body.\n    :::\n", 1),
        ];

        for (input, line) in fixtures {
            assert_eq!(
                unwrap_markdown(input),
                Err(FormatError::UnsupportedCustomContainer { line })
            );
        }
    }

    #[test]
    fn permits_custom_container_markers_inside_literal_blocks() {
        let input = "---\nexample: |\n  :::note\n---\n\n```text\n:::note\n```\n\n<div>\n:::note\n</div>\n\nWrapped\nprose.\n";
        let expected = "---\nexample: |\n  :::note\n---\n\n```text\n:::note\n```\n\n<div>\n:::note\n</div>\n\nWrapped prose.\n";

        assert_eq!(unwrap(input), expected);
    }

    #[test]
    fn retains_crlf_and_missing_final_newline() {
        assert_eq!(unwrap("First\r\nsecond\r\n"), "First second\r\n");
        assert_eq!(unwrap("First\nsecond"), "First second");
    }

    #[test]
    fn leaves_documents_without_soft_breaks_byte_identical() {
        let input = "# Already wide\r\n\r\n- one\r\n- two\r\n";

        assert_eq!(unwrap(input), input);
    }

    #[test]
    fn collapses_whitespace_at_a_soft_break() {
        assert_eq!(unwrap("First \n   second\n"), "First second\n");
    }

    #[test]
    fn unwraps_before_reflowing_plain_prose() {
        let input = "This paragraph\nhas enough words to wrap cleanly.\n";
        let expected = "This paragraph has\nenough words to wrap\ncleanly.\n";

        assert_eq!(format_at_width(input, 20), expected);
    }

    #[test]
    fn reflows_nested_task_items_with_container_prefixes() {
        let input = "> - [x] This nested task has enough words to wrap cleanly across lines.\n";
        let expected =
            "> - [x] This nested task has\n>   enough words to wrap cleanly\n>   across lines.\n";

        assert_eq!(format_at_width(input, 32), expected);
    }

    #[test]
    fn keeps_protected_blocks_and_headings_unchanged_at_a_small_width() {
        let input = "# A deliberately very long heading that stays intact\n\n[reference]: https://example.com/a/very/long/destination\n\n```text\na very long line inside a code fence\n```\n\n| long header | another header |\n| --- | --- |\n| long value | another value |\n";

        assert_eq!(format_at_width(input, 16), input);
    }

    #[test]
    fn does_not_split_inline_code_spans_or_long_words() {
        let input = "Before `a very long inline code span` after supercalifragilisticexpialidocious words.\n";
        let expected = "Before\n`a very long inline code span`\nafter\nsupercalifragilisticexpialidocious\nwords.\n";

        assert_eq!(format_at_width(input, 20), expected);
    }

    #[test]
    fn preserves_explicit_hard_break_spaces_when_reflowing() {
        let input = "One two three four five six seven.  \nNext line.\n";
        let expected = "One two three four\nfive six seven.  \nNext line.\n";

        assert_eq!(format_at_width(input, 18), expected);
    }

    #[test]
    fn measures_unicode_display_columns_and_preserves_crlf() {
        let input = "界界 alpha beta gamma\r\n";
        let expected = "界界 alpha\r\nbeta gamma\r\n";

        assert_eq!(format_at_width(input, 10), expected);
    }

    #[test]
    fn width_formatting_is_idempotent() {
        let input =
            "- A list item with enough words to be wrapped more than once by the formatter.\n";
        let once = format_at_width(input, 28);

        assert_eq!(format_at_width(&once, 28), once);
    }
}
