//! Markdown-aware removal of soft line breaks.

use pulldown_cmark::{Event, Options, Parser};

/// Removes soft line breaks from Markdown prose while leaving every other
/// source byte untouched.
///
/// Markdown's parser decides which newlines are soft breaks. Newlines that
/// define block structure, fenced or indented code, tables, metadata, raw HTML,
/// or explicit hard breaks are therefore preserved without bespoke rewriting.
pub fn unwrap_markdown(input: &str) -> String {
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
    use super::unwrap_markdown;

    #[test]
    fn joins_soft_wrapped_paragraphs() {
        let input = "This paragraph was wrapped at\neighty columns and should become\none physical line.\n\nAnother paragraph.\n";
        let expected = "This paragraph was wrapped at eighty columns and should become one physical line.\n\nAnother paragraph.\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn preserves_headings_rules_and_unwraps_setext_headings() {
        let input = "# Heading\n\nA wrapped setext\nheading\n=======\n\n---\n\nText\n";
        let expected = "# Heading\n\nA wrapped setext heading\n=======\n\n---\n\nText\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn preserves_fenced_and_indented_code() {
        let input = "Before this\ncode.\n\n```rust\nlet value =\n    42;\n```\n\n    indented();\n    still_indented();\n";
        let expected = "Before this code.\n\n```rust\nlet value =\n    42;\n```\n\n    indented();\n    still_indented();\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn unwraps_nested_list_items_without_merging_items() {
        let input = "- A list item that was\n  wrapped across lines.\n- A sibling item.\n    - A nested item that is\n      wrapped too.\n\n1. An ordered item that\n   also wraps.\n2. Another item.\n";
        let expected = "- A list item that was wrapped across lines.\n- A sibling item.\n    - A nested item that is wrapped too.\n\n1. An ordered item that also wraps.\n2. Another item.\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn unwraps_matching_blockquotes_and_gfm_alerts() {
        let input = "> A quoted paragraph that\n> was wrapped.\n>\n> [!NOTE]\n> An alert paragraph that\n> was wrapped.\n";
        let expected = "> A quoted paragraph that was wrapped.\n>\n> [!NOTE]\n> An alert paragraph that was wrapped.\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn preserves_explicit_hard_breaks() {
        let input = "First line.  \nSecond line that\ncan still unwrap.\n\nBackslash break.\\\nLast line.\n";
        let expected =
            "First line.  \nSecond line that can still unwrap.\n\nBackslash break.\\\nLast line.\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn preserves_gfm_tables() {
        let input = "| Name | Meaning |\n| --- | --- |\n| wide-md | Unwraps text |\n\nA wrapped paragraph\nafter the table.\n";
        let expected = "| Name | Meaning |\n| --- | --- |\n| wide-md | Unwraps text |\n\nA wrapped paragraph after the table.\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn preserves_metadata_raw_html_and_display_math() {
        let input = "---\ntitle: A document\ndescription: Kept\nas written\n---\n\n<div>\nraw lines\nstay separate\n</div>\n\n$$\na +\nb\n$$\n\nWrapped\nprose.\n";
        let expected = "---\ntitle: A document\ndescription: Kept\nas written\n---\n\n<div>\nraw lines\nstay separate\n</div>\n\n$$\na +\nb\n$$\n\nWrapped prose.\n";

        assert_eq!(unwrap_markdown(input), expected);
    }

    #[test]
    fn retains_crlf_and_missing_final_newline() {
        assert_eq!(unwrap_markdown("First\r\nsecond\r\n"), "First second\r\n");
        assert_eq!(unwrap_markdown("First\nsecond"), "First second");
    }

    #[test]
    fn leaves_documents_without_soft_breaks_byte_identical() {
        let input = "# Already wide\r\n\r\n- one\r\n- two\r\n";

        assert_eq!(unwrap_markdown(input), input);
    }

    #[test]
    fn collapses_whitespace_at_a_soft_break() {
        assert_eq!(unwrap_markdown("First \n   second\n"), "First second\n");
    }
}
