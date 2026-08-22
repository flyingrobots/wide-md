# wide-md

`wide-md` removes artificial hard wrapping from Markdown. It reads one Markdown document from standard input and writes the unwrapped document to standard output.

```console
$ wide-md < narrow.md > wide.md
```

Or in a pipeline:

```console
$ cat narrow.md | wide-md
```

The filter joins ordinary soft line breaks inside paragraphs, list items, and block quotes. It preserves blank lines and syntax-significant newlines, including fenced and indented code, tables, headings, thematic breaks, front matter, raw HTML blocks, and explicit Markdown hard breaks.

## Install

With a Rust toolchain installed:

```console
$ cargo install --path .
```

## Example

Input:

```markdown
This paragraph has been manually wrapped
even though Markdown can render it at whatever
width the reader has available.

- List items can be wrapped
  too.
```

Output:

```markdown
This paragraph has been manually wrapped even though Markdown can render it at whatever width the reader has available.

- List items can be wrapped too.
```
