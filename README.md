# wide-md

`wide-md` removes artificial hard wrapping from Markdown. It can act as a stdin/stdout filter, format individual files, or recursively format directories.

The formatter uses `pulldown-cmark` with a built-in extended Markdown profile to distinguish ordinary soft line breaks from syntax-significant newlines. Covered structures such as fenced and indented code, tables, headings, front matter, raw HTML, math, link definitions, and explicit hard breaks are left intact. Known unsupported constructs are refused when they can be identified safely rather than guessed at as prose.

## Install

With a Rust toolchain installed:

```console
$ cargo install --path .
```

## Usage

With no path arguments, `wide-md` retains its original filter behavior:

```console
$ wide-md < narrow.md > wide.md
$ cat narrow.md | wide-md
$ cat narrow.md | wide-md -
```

File and directory arguments are formatted in place:

```console
$ wide-md README.md
$ wide-md README.md CHANGELOG.md docs/
$ wide-md docs/
```

Directory traversal processes `.md` and `.markdown` files recursively. It respects hidden-file rules, `.ignore`, `.gitignore`, global Git ignores, and repository excludes. Symlinks are never followed, and `.git` directories are always skipped.

Use `--include` to add other filename patterns. The flag can be repeated:

```console
$ wide-md --include='*.mkd' --include='*.mdown' docs/
```

`--include` controls file discovery only; it does not add support for another Markdown dialect.

Use `--no-ignore` to include hidden and ignored files while still excluding `.git` directories:

```console
$ wide-md --no-ignore docs/
```

Explicit file arguments are processed regardless of their extension or ignore status, except that `.mdx` files are refused because MDX is not supported safely.

## Input boundary

Input must be UTF-8 Markdown. A leading UTF-8 byte-order mark is preserved byte-for-byte and excluded from parsing so that following front matter remains recognizable.

MDX is not supported. Whenever an `.mdx` path reaches formatting—including through an explicit path, an include glob during directory traversal, or `--stdout`—it is refused. Standard input has no filename or dialect metadata and is treated as Markdown; do not pipe MDX into `wide-md`.

Custom `:::` containers and directives are also not supported yet. When a `:::` marker begins a Markdown content line, including inside a blockquote or list item, `wide-md` exits with status 2 and leaves that file unchanged. Literal `:::` text inside front matter, code blocks, and raw HTML blocks recognized by the built-in parser remains allowed.

These checks cover known unsafe boundaries; they are not automatic recognition of every Markdown-derived language. Preview unfamiliar dialects with `--diff` before using write mode.

## Width

Without `--width`, every parser-identified soft break is removed, leaving one physical source line per Markdown paragraph.

With `--width`, prose is first unwrapped and then reflowed to the requested number of Unicode display columns:

```console
$ wide-md --width=120 README.md docs/
```

List and blockquote prefixes are reproduced on continuation lines. Individual words, URLs, inline code spans, and inline links are not split; one of those tokens may therefore exceed the requested width. Protected Markdown structures are never reflowed merely to satisfy the width.

## Non-writing modes

Check files without changing them:

```console
$ wide-md --check .
```

`--check` exits with status 1 when any file would change, making it suitable for CI.

Preview unified diffs without changing files:

```console
$ wide-md --diff README.md docs/
```

Format one file to stdout without changing it:

```console
$ wide-md --stdout README.md
$ wide-md --width=120 --stdout README.md > README.preview.md
```

`--check`, `--diff`, and `--stdout` are mutually exclusive. Standard input (`-`) cannot be mixed with filesystem paths because there is no unambiguous multi-document stdout representation.

## Parallelism

Filesystem paths are processed in parallel. Set an explicit worker count when useful:

```console
$ wide-md --jobs=4 docs/
```

Discovery and reporting remain deterministic regardless of the worker count.

## Configuration

`wide-md` searches the current directory and then its ancestors for the nearest `.wide-md.toml`. The file can provide repository-wide defaults:

```toml
width = 120
include = ["*.mkd", "*.mdown"]
jobs = 4
no-ignore = false
```

Command-line `--width` and `--jobs` values override their configured values. Command-line `--include` patterns are added to configured patterns. `--no-ignore` enables traversal of ignored files even when the configuration leaves it disabled.

Unknown configuration keys, invalid globs, and zero values for `width` or `jobs` are errors rather than silently ignored settings.

## Writes and reporting

Changed files are written to temporary files in the same directory, flushed and synchronized, assigned the original file permissions, and atomically renamed over the originals. Files whose output is byte-identical are not rewritten. Atomic replacement preserves permissions but intentionally gives the changed file a new modification time; platform-specific extended attributes and hard-link identity are not promised.

Processing is atomic per changed file, not across an entire multi-file invocation. If one file fails, another valid file may already have been changed before the command exits with status 2.

Path modes print only a final summary and errors to stderr, leaving stdout available for `--diff` and `--stdout`.

Exit statuses are:

| Status | Meaning |
|---:|---|
| `0` | Formatting succeeded, a diff was produced successfully, or `--check` found no changes |
| `1` | `--check` found files that would change |
| `2` | Invalid usage, unsupported input, configuration failure, discovery failure, or file-processing failure |

## Example

Input:

```markdown
This paragraph has been manually wrapped
even though Markdown can render it at whatever
width the reader has available.

- List items can be wrapped
  too.
```

Default output:

```markdown
This paragraph has been manually wrapped even though Markdown can render it at whatever width the reader has available.

- List items can be wrapped too.
```

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
