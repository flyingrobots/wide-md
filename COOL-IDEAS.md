# COOL IDEAS for `wide-md`

Status: living design backlog, not an implementation commitment. The initial assessment is anchored to `wide-md` commit `3f9597d` and to a real run against `/Users/james/git/blog/jim-component-ownership.md` on 2026-08-22. Shipped-safeguard status is reconciled through `52a599b` on 2026-08-23.

## Short answer

The core idea is good and should stay small: let a Markdown parser identify actual soft breaks, remove those breaks, and leave syntax-significant source alone. The current stdin filter, file and directory modes, ignore handling, deterministic reporting, idempotence, and same-directory atomic replacement already make a strong foundation.

The next work should remain about **trust before reach**. The baseline probes at `3f9597d` found three source-corrupting cases: a UTF-8 BOM defeated front-matter recognition, advertised `*.mdx` inclusion merged separate ESM statements, and custom `:::` containers were flattened. Those immediate hazards are now closed: BOMs are preserved, filename-bearing MDX input is refused, and custom containers outside parser-recognized literal blocks are refused. Multi-file preflight, semantic equivalence, and compare-before-replace protection remain more important than convenience flags.

My proposed product contract is:

> `wide-md` rewrites a file only when it recognizes the file's dialect, can classify every changed newline as safely replaceable, and can verify that the formatted document has the same meaning under that dialect. Otherwise it leaves the file byte-identical and reports an actionable diagnostic.

## Recommended order

1. **P0 — Close the remaining correctness holes:** multi-file preflight and compare-before-replace race protection.
2. **P0 — Add a semantic safety gate:** canonical parser-event equivalence around every proposed rewrite, calibrated by behavior-breaking mutations.
3. **P1 — Make decisions inspectable:** config provenance, per-file config resolution, `--explain`, changed-path output, transformation statistics, and stable JSON reports.
4. **P1 — Give authors escape hatches:** ignore-next and off/on regions for intentional source lineation or unsupported embedded syntax.
5. **P1 — Harden writes and portability:** hard-link detection, metadata policy, fault injection, and macOS/Linux/Windows tests.
6. **P2 — Improve width mode:** parser-derived inline atoms, reference-link/image handling, grapheme-aware width, real tab stops, and explicit newline policy.
7. **P2 — Package the tool:** CI, release binaries, checksums, Homebrew/Cargo installation, pre-commit, and editor integration.

Shipped safety baseline:

- `e15d3d6` made both public formatting entry points return `Result<String, FormatError>`, preserved a leading BOM around parsing and rewriting, rejected filename-bearing `.mdx` input, and refused `:::` markers outside recognized literal blocks.
- `2692a98` made literal-block exclusion linear instead of rescanning every parser range for every source line.
- `cac28a1` closed nested-container, bare-CR container, and exact-dotfile `.mdx` evasions.
- `52a599b` aligned parser and width-reflow handling for bare-CR line endings without changing source-offset correspondence.

## What already works well and should remain

- The parser, rather than a regular expression, decides which CommonMark soft breaks are candidates for removal (`src/lib.rs#36@3f9597d`).
- Code blocks, tables, metadata blocks, headings, raw HTML blocks, math, rules, and reference definitions receive explicit protection in width mode (`src/lib.rs#102@3f9597d`).
- Default mode and width mode are idempotent in the covered cases.
- Explicit hard breaks, CRLF input, missing final newlines, blockquotes, nested lists, task items, footnotes, and Unicode display columns have focused tests.
- `--check`, `--diff`, and `--stdout` provide useful non-writing workflows.
- Directory discovery respects standard ignore sources, never follows symlinks, skips `.git`, deduplicates inputs, and reports deterministically.
- Unchanged files are not rewritten.
- Changed files use a temporary file in the same directory, sync the file, atomically rename it, and sync the parent directory (`src/cli.rs#608@3f9597d`). That is unusually careful for a small formatter.
- Configuration rejects unknown keys and invalid zero values rather than guessing.
- Human summaries stay on stderr, leaving stdout usable for document or diff output.

The aim of the ideas below is to preserve those properties while making the formatter's safety boundary explicit and executable.

## Evidence from the first real document

Running default mode against `jim-component-ownership.md` produced a useful result:

- `589` physical lines became `410`.
- The file became byte-stable on the immediate second run.
- UTF-8 encoding, final LF, and mode `0640` were preserved.
- The test suite passed all `34` tests before the run.
- The resulting document contains `65` lines longer than 200 source characters, with a longest observed line of 560 characters.

That validates the central behavior, but it also exposes an important product trade-off. One-line paragraphs are excellent when the editor performs visual wrapping and the author wants to eliminate arbitrary source widths. They are less pleasant in terminals, line-oriented review tools, blame, and small patches to long paragraphs. `--width=100` or `--width=120` already offers the right alternative; the documentation should present these as two deliberate authoring policies rather than implying that unlimited width is universally best.

The shell's whitespace-delimited word count also fell by eight. That metric counts standalone Markdown container markers as words, and repeated blockquote markers can disappear when continuation lines are joined, so the count is not a semantic-word invariant. A transformation report would show the exact reasons instead of leaving the user to infer them.

## P0: shipped safety baseline and remaining gaps

### 1. Preserve a UTF-8 BOM before parsing — shipped

Historical probe at `3f9597d`:

```markdown
<UTF-8 BOM>---
title: Test
description: two
 lines
---
```

Historical output at `3f9597d` began like this:

```markdown
<UTF-8 BOM>--- title: Test description: two lines
---
```

At that baseline, the BOM prevented the opening delimiter from being recognized as YAML metadata, so parser-identified soft breaks flattened the front matter.

Shipped behavior:

- `e15d3d6` detects a leading `EF BB BF`, parses the body without it, and restores it byte-for-byte around both default and width formatting.
- Focused tests cover YAML and plus-delimited metadata, ordinary Markdown, CRLF, and BOM-only input.
- No-op output remains byte-identical, including the BOM.

The original acceptance criterion is met: no supported metadata block becomes prose solely because a leading UTF-8 BOM precedes it. Explicit policy for other leading control characters or NUL bytes remains separate hardening work.

### 2. Keep filename inclusion separate from dialect support — refusal shipped

The README at `3f9597d` demonstrated:

```console
$ wide-md --include='*.mdx' --include='*.mkd' docs/
```

`--include` only changes discovery. It does not add an MDX parser or protect MDX ESM and expression syntax. Historical probe at `3f9597d`:

```mdx
import Alpha from './alpha'
export const value = 1
```

Historical output at `3f9597d`:

```mdx
import Alpha from './alpha' export const value = 1
```

That historical output was invalid JavaScript/MDX. The baseline `starts_with("import ")` and `starts_with("export ")` checks ran during width reflow, but formatting unwrapped first (`src/lib.rs#21@3f9597d` and `src/lib.rs#329@3f9597d`), so the protection arrived too late.

Shipped behavior:

- `e15d3d6` removed `*.mdx` from the README example and refuses extension-bearing MDX in explicit, discovered, and `--stdout` paths before formatting.
- `cac28a1` also refuses the filename that is exactly `.mdx`, for which `Path::extension()` is absent.
- CLI tests prove rejected files remain byte-identical and exit with status 2.

If an expert escape hatch is ever desired, name it honestly, such as `--dialect=commonmark --allow-unknown-extension`, rather than treating `--include` as consent to semantic risk.

Long-term support options:

- Add a real MDX-aware parser/tokenizer that protects ESM blocks, JSX, expressions, and embedded JavaScript/TypeScript before Markdown reflow.
- Or keep `wide-md` Markdown-only and leave MDX to a separate frontend that calls a stable edit API for Markdown regions.

Do not grow an open-ended list of line-prefix heuristics. Multiline ESM, comments, strings, braces, TypeScript syntax, and JSX expression bodies make that approach impossible to close convincingly.

The refusal-side acceptance criterion is met for filename-bearing MDX: documentation does not present it as supported, and the CLI refuses it. Actual MDX support remains open and requires a dialect-aware frontend plus structural fixtures.

### 3. Treat custom containers and directives as a dialect, not prose — refusal shipped

Historical probe at `3f9597d`:

```markdown
:::note
A wrapped
container body.
:::
```

Historical output at `3f9597d`:

```markdown
:::note A wrapped container body.
:::
```

At that baseline, the opening fence and body were merged because the `:::` width guard ran after the destructive unwrapping pass.

Shipped behavior:

- `e15d3d6` preflights `:::` markers before unwrapping and returns `FormatError::UnsupportedCustomContainer` outside parser-recognized metadata, code, and raw HTML blocks.
- `2692a98` keeps the preflight linear in source lines plus merged literal ranges.
- `cac28a1` closes nested list, definition-list, footnote, blockquote, and bare-CR evasions, with focused regressions.

The default-profile acceptance criterion is met for recognized `:::` markers: they are refused rather than treated as prose. Named MyST, MkDocs, or VitePress support remains open; those grammars need complete opener/body/closer fixtures, nesting rules, attributes, and unterminated-container behavior before admission.

### 4. Verify semantic equivalence before every write

Parser-guided editing is safer than text heuristics, but using the parser to locate `SoftBreak` events does not by itself prove that the output parses the same way. Add a second gate after formatting and before reporting a file as changeable. A gate implemented with the same parser is useful consistency evidence, not an independent witness; a second renderer or parser can add independence for profiles where that comparison is well defined.

A practical first version can canonicalize both parser event streams:

- Remove source offsets.
- Coalesce adjacent text events.
- Canonicalize a soft break to the single rendered space that replaces it.
- Retain every structural start/end tag, code payload, HTML payload, link destination, metadata payload, hard break, rule, math event, task marker, and footnote relationship.
- Compare the canonical streams and reject a mismatch.

For supported profiles, a renderer-level comparison can be a useful second witness, but it should not replace structural comparison. Equal HTML may hide source constructs that downstream tools care about, and raw HTML/MDX complicates normalization.

On failure, report:

- The file.
- The smallest original and formatted source ranges around the first mismatch.
- The parser events that diverged.
- The selected dialect and configuration source.
- A recommendation to use an ignore directive or file an issue with a minimized reproducer.

The gate should be enabled by default in write, check, diff, and stdout modes. A bypass, if one exists at all, should be conspicuous and should never be implied by `--include`.

Mutation calibration is essential. Tests should prove the gate catches implementations that deliberately:

- Remove one list marker.
- Convert an explicit two-space or backslash hard break into a space.
- Join an MDX import and export.
- Alter a code fence payload.
- Move text into or out of a link.
- Flatten front matter.
- Merge adjacent blockquotes or list items.

Acceptance criterion: every accepted rewrite has executable evidence that only the formatter's declared source-layout differences occurred.

### 5. Detect and narrow lost-update races between read and rename

The write path reads a file, formats it, and later atomically replaces the path. Another process can change the original after the read and before `persist(path)`, causing the formatter to overwrite newer content. Atomic rename prevents torn files; it does not provide compare-and-swap semantics.

Recommended behavior:

- Capture a content digest and stable identity metadata when reading.
- Immediately before replacement, reopen the path without following symlinks and verify the identity/content still matches.
- If it changed, leave the newer file untouched and report `source changed while formatting`.
- Use directory-relative handles, no-follow operations, and advisory locking where available to narrow path-substitution and cooperative-writer races.
- Investigate platform atomic-swap APIs if stronger detection is worth the complexity, while documenting their crash and rollback limits.
- Test controlled concurrent writers before the pre-replace check and as close to replacement as the harness can schedule them.

There is no portable, perfect compare-and-swap rename against an uncooperative writer. Do not claim that a digest recheck eliminates the final check-to-rename window. Acceptance criterion: known and cooperative races are detected, the window is minimized, and the residual platform-specific nonclaim is explicit.

### 6. Make multi-file write behavior fail closed

Current directory processing can modify valid files even when discovery or processing of another path fails; it then exits `2`. Per-file atomicity is not batch atomicity. A command such as `wide-md good.md missing.md` should not quietly change `good.md` before reporting that the invocation failed unless the user explicitly selected best-effort behavior.

Recommended default:

1. Discover every input.
2. Read and format every candidate.
3. Run semantic verification for every changed candidate.
4. Validate write preconditions for every candidate.
5. Only then begin replacements.

This eliminates partial changes caused by discovery, decoding, parsing, or verification errors. Replacement itself can still fail midway, so reporting must remain honest. Full crash-safe multi-file transactions would require a journal/rollback protocol and may be disproportionate for this tool.

If current behavior is useful for very large trees, expose it explicitly as `--best-effort` and report every successfully written path.

Acceptance criteria:

- A test with one valid file and one missing/unreadable/invalid file leaves the valid file unchanged by default.
- A write failure after some replacements reports the exact changed and unchanged paths; it never claims the batch was atomic.
- `--check` and `--diff` remain naturally all-read/non-writing.

## P1: make behavior inspectable

### 7. Resolve configuration from the target, not accidentally from the shell

Configuration is currently discovered only from the process current directory and its ancestors (`README.md#96@3f9597d`). An absolute file in another repository therefore receives the caller's configuration, not the target repository's. The real blog run had to execute with `/Users/james/git/blog` as the working directory to avoid this ambiguity.

Recommended model:

- For each file, search from that file's parent toward its repository/filesystem boundary for the nearest `.wide-md.toml`.
- Cache resolutions so directory runs remain cheap.
- Allow one invocation to group files by config when paths span repositories.
- Add `--config PATH` to force one configuration, `--no-config` to force built-in defaults, and `--show-config` or `--explain-config FILE` to print the effective values and provenance.
- Add `--stdin-filepath PATH` so editors can send content on stdin while still selecting the right config, dialect, and filename rules.
- Define whether search stops at a VCS root. Prefer a repository-local config plus a separately named XDG/global config over accidentally inheriting any `.wide-md.toml` found arbitrarily high in the filesystem.
- Add `version = 1` to the config schema before it expands materially.

Example diagnostic:

```text
jim-component-ownership.md: width=120 (blog/.wide-md.toml:2), dialect=gfm (default), line-ending=preserve (default)
```

Acceptance criterion: a user can always ask which configuration controlled a file, and invoking the same file by absolute path from another directory does not silently change the result.

### 8. Add author-controlled protected regions

Markdown intentionally gives ordinary soft breaks the same rendered meaning as spaces. A parser therefore cannot know whether an author used those breaks as disposable hard wrapping, one-sentence-per-line source structure, poetry-like source layout without hard breaks, or a convention consumed by another source tool.

Add explicit, structure-aware directives:

```markdown
<!-- wide-md: ignore-next -->
This source block stays
exactly as authored.

<!-- wide-md: off -->
Everything in this region stays byte-identical.
<!-- wide-md: on -->
```

Possible front matter control:

```yaml
wide-md: false
```

Rules:

- Recognize directives only as Markdown HTML comments, never inside code fences or raw strings.
- Diagnose unmatched, nested, or malformed off/on markers.
- Preserve the protected range byte-for-byte in every mode.
- Include protected ranges in `--explain` output.
- Consider an `ignore-next-paragraph` scope rather than the ambiguous word `block`, because dialects disagree on custom block syntax.

Acceptance criterion: intentional source lineation has a local, reviewable escape hatch that does not require excluding the entire file.

### 9. Report paths and transformations, not only totals

The current summary is excellent for a quiet happy path but too sparse for automation or trust-sensitive bulk writes. Add orthogonal reporting controls:

- `--list-different`: one changed path per line, useful with `--check`.
- `--verbose`: one deterministic human-readable result per file.
- `--stats`: soft breaks removed, paragraphs changed, lines before/after, maximum display width before/after, protected constructs encountered, warnings, and elapsed time.
- `--report=json`: a versioned machine-readable result with configuration provenance, changed paths, per-file digests, edit ranges, warnings, failures, and aggregate counts.
- `--quiet`: suppress the human summary when exit status is sufficient.
- `--explain FILE` or `--explain FILE:LINE`: show why a newline was joined or protected.
- `--color=auto|always|never`: color human diffs without contaminating machine output.

The library should expose the same data rather than forcing the CLI to reverse-engineer it from before/after strings:

```rust
pub struct FormatReport {
    pub output: String,
    pub edits: Vec<Edit>,
    pub protected_ranges: Vec<ProtectedRange>,
    pub warnings: Vec<Diagnostic>,
}
```

Keep stdout contracts unambiguous. Document output belongs on stdout; human diagnostics belong on stderr; JSON report mode should own stdout unless a separate report path is explicitly supplied.

Acceptance criterion: an agent or CI job can establish exactly which files changed, why, under which config, and from/to which digests without scraping prose.

### 10. Make unified diffs portable and patch-friendly

An explicit path outside the current directory is displayed as an absolute path. Prefixing it with `a/` and `b/` can produce labels such as `a//Users/...`, which are awkward and may not apply as a normal patch.

Ideas:

- Anchor labels to each target's repository root when known.
- Add `--relative-to PATH` for deterministic labels.
- Quote unusual paths using a documented patch-compatible convention.
- Decide whether multiple repositories in one diff are supported or rejected.
- Add `--diff-exit-code` only if users need diff mode to return `1` on changes; keep current documented exit behavior otherwise.

Acceptance criterion: `wide-md --diff` output for in-repo files can be consumed by `git apply --check` in fixtures with spaces, Unicode, and absolute invocation paths.

## P1: harden filesystem behavior

### 11. Detect hard links and define metadata preservation

Atomic replacement intentionally creates a new inode. The README correctly disclaims extended attributes and hard-link identity, but silently breaking either can still surprise users, especially on macOS.

Recommended defaults:

- Detect a link count greater than one and refuse to rewrite unless the user explicitly chooses to break link identity.
- Preserve mode, ownership where permitted, ACLs, file flags, and extended attributes when the platform offers reliable APIs.
- If a metadata class cannot be preserved, report that before the write, not after losing it.
- Keep changed modification time as the default; it truthfully records a content change.
- Document creation-time behavior separately because support varies.
- Never follow a symlink while copying metadata.

Potential flags:

```text
--metadata=preserve     # safe default
--metadata=basic        # mode only, explicit opt-in
--break-hardlinks       # explicit opt-in
```

Tests should cover mode, ACL/xattr where available, ownership behavior, hard links, symlinks, immutable flags, and failure cleanup.

Acceptance criterion: replacement cannot silently discard a metadata class that the tool claimed it would preserve.

### 12. Fault-inject every write phase

The write sequence deserves tests at the failure boundaries, not just a successful permission check:

- Temporary creation failure.
- Short/failed write.
- Flush failure.
- File sync failure.
- Permission/metadata copy failure.
- Source-changed comparison failure.
- Rename/persist failure.
- Parent-directory sync failure after rename.
- Process interruption with a staged temp file.

The result should distinguish `content not replaced` from `content replaced but durability confirmation failed`; those states have different remediation.

Add platform CI because directory syncing and replace-over-existing semantics differ on macOS, Linux, and Windows. The current Unix permission test is necessary but not sufficient.

Acceptance criterion: every write-phase error has a deterministic diagnostic, cleanup expectation, and test asserting the surviving bytes.

## P1/P2: define supported Markdown dialects

### 13. Replace “CommonMark plus everything” with named profiles

The parser currently enables tables, footnotes, strikethrough, task lists, heading attributes, two metadata syntaxes, math, GFM, definition lists, superscript, subscript, and wikilinks simultaneously (`src/lib.rs#493@3f9597d`). Calling this simply “a CommonMark parser” hides a material part of the behavior.

Add explicit profiles, for example:

- `commonmark`: only CommonMark core.
- `gfm`: CommonMark plus GitHub tables, task lists, strikethrough, autolinks, and alerts covered by fixtures.
- `extended`: the current broad pulldown-cmark option set, preserved for compatibility.
- Future named profiles for specific ecosystems only when their non-CommonMark blocks are genuinely protected.

Configuration:

```toml
version = 1
dialect = "gfm"
width = 120
```

Do not auto-detect dialect from content and silently switch behavior. Filename conventions or config may select a profile; ambiguous unsupported constructs should generate diagnostics.

Acceptance criterion: every parser option is attributable to a named profile, and the selected profile appears in explain/JSON output.

### 14. Build protected spans from parser structure, not a second mini-parser

Width mode manually recognizes code spans, angle-bracket constructs, and inline links (`src/lib.rs#341@3f9597d`). This duplicates syntax rules and is already narrower than the configured parser. A confirmed width-mode probe split a reference-style image label across lines:

```markdown
Before ![an image
label with
spaces][image] after
```

That may remain equivalent under the current parser, but it creates unnecessary compatibility risk with other Markdown consumers and makes source harder to scan.

Recommended architecture:

- Derive indivisible or internally wrappable spans from parser events and source offsets.
- Wrap only at source whitespace known to be outside protected atoms.
- Cover inline and reference links/images, code, autolinks, HTML, entities, footnote references, math, wikilinks, emphasis delimiters where needed, and extension-specific inline syntax.
- Preserve link/image labels as one source atom by default even if the selected dialect technically permits soft breaks inside them.
- Remove manual scanners once event-derived spans have equivalent coverage.

Acceptance criterion: adding a parser-supported inline construct requires one structural mapping and fixtures, not another ad hoc delimiter scanner.

### 15. Protect only the necessary span in width mode

Current width protection is line-wide: if any protected range intersects a line, the entire line is left alone. A short inline formula inside a 500-character paragraph can therefore prevent the entire paragraph from respecting `--width`.

Teach the wrapper to treat protected inline spans as unsplittable atoms while still wrapping surrounding prose. Preserve whole-line protection for headings, tables, block HTML, metadata, and other truly structural blocks.

Acceptance criterion: a paragraph containing inline math or HTML can wrap around that construct without changing its bytes or exceeding the requested width except when the protected atom itself is wider.

## P2: width, Unicode, and newline policy

### 16. Measure terminal columns with string/grapheme context

Width is currently the sum of `UnicodeWidthChar` values, with every tab counted as four columns (`src/lib.rs#443@3f9597d`). Per-character sums can mismeasure emoji ZWJ sequences and other grapheme-context cases. Tabs occupy the next tab stop, not always four columns.

Ideas:

- Use a string/grapheme-aware display-width implementation that handles emoji sequences as a unit.
- Calculate tabs from the current column and make `tab-width` configurable, defaulting to four.
- Keep Markdown indentation semantics separate from terminal display width; converting structural tab prefixes to spaces can change parsing.
- Add fixtures for combining marks, CJK, ambiguous-width characters, emoji modifiers, flags, ZWJ families, tabs after different prefixes, and RTL text.
- Document that display width is a deterministic tool policy, not a promise to match every terminal/font.

Acceptance criterion: width calculations are internally consistent, idempotent, and tested at grapheme and tab-stop boundaries.

### 17. Make line-ending behavior explicit

The formatter preserves common all-LF and all-CRLF inputs in covered cases, but mixed line endings can be normalized incidentally when the only CRLF is a removed soft break. Choose and expose a policy:

```toml
line-ending = "preserve" # default
# "lf" and "crlf" are explicit normalization choices
```

For `preserve`:

- Retain each surviving original line ending.
- Choose inserted endings from the containing paragraph's local style.
- Warn or fail on mixed styles if local preservation would be ambiguous.
- Preserve final-newline presence exactly unless normalization was explicitly requested.
- Report any line-ending normalization separately from Markdown reflow.

Acceptance criterion: ordinary formatting never performs an undisclosed newline normalization.

### 18. Offer named source-layout policies

The current behavior maps naturally to named policies:

- `layout = "paragraph"`: current unlimited-width default.
- `layout = "width"` plus `width = 120`: current bounded reflow.
- Potential future `layout = "sentence"`: one sentence per source line, only if a language-agnostic and stable contract can be defined.
- `layout = "preserve"`: useful as a validation-only profile but not a formatter.

Names would improve config readability and leave room for future behavior without overloading absence/presence of `width`. Keep CLI compatibility with `--width`.

I would not implement sentence mode soon. Abbreviations, code, links, locale, and deliberate sentence fragments make it a much larger product than Markdown reflow.

### 19. Add exclusion and size controls

Directory traversal can include extra patterns but cannot exclude an otherwise discovered Markdown subtree except through repository ignore files.

Useful additions:

- Repeatable `--exclude GLOB` and `exclude = []` config.
- `--max-file-size` with a conservative default warning or explicit failure policy for unusually large files.
- Per-pattern dialect/layout overrides only if the config remains legible.
- `wide-md files` or `--list-files` to preview discovery without formatting.

Never allow excludes to re-enable `.git`, and keep explicit file behavior clearly documented.

Acceptance criterion: users can inspect and bound the exact formatting set before a large run.

## P2: verification depth

### 20. Adopt the CommonMark and GFM corpora

Use the official CommonMark spec examples and an authoritative GFM fixture set as data-driven tests for each declared profile. The goal is not merely “does parsing succeed?” but the formatter invariants:

- Formatting is idempotent.
- Canonical semantic events are unchanged.
- Protected source ranges remain byte-identical.
- Only declared whitespace/container-prefix edits occur.
- Width mode respects its bound unless an indivisible atom exceeds it.
- A byte-identical result does not rewrite the file.

Keep real-world sanitized fixtures for front matter, documentation generators, GitHub alerts, nested containers, and documents mixing many constructs. Do not make one private article the sole evidence.

### 21. Add fuzzing and property tests

Good fuzz targets:

- Arbitrary UTF-8 into `format_markdown` never panics.
- `format(format(input)) == format(input)` for every layout/dialect.
- Accepted output is canonically equivalent to input.
- Source-offset edits are ordered, non-overlapping, and on UTF-8 boundaries.
- Protected ranges are byte-identical.
- Width wrapping terminates and does not drop/duplicate non-whitespace tokens.
- CRLF/BOM/final-newline policies hold under arbitrary combinations.

Seed fuzzers with the spec corpus and every historical bug. Run a bounded fuzz smoke test in CI and longer campaigns separately.

### 22. Calibrate public behavior tests with mutations

For each critical guarantee, temporarily introduce a named behavior-breaking mutation and prove a public-boundary test fails. Examples:

- Join a hard break.
- Drop a repeated blockquote marker incorrectly.
- Rewrite code-fence content.
- Lose the BOM.
- Accept non-equivalent parser events.
- Overwrite a concurrently changed file.
- Modify one valid file when another batch input fails preflight.

Record the mutation and the test that kills it. This prevents tautological tests that merely mirror the implementation.

### 23. Benchmark the actual bottlenecks

Measure:

- One small file startup time.
- Large single-document parsing and formatting.
- A repository with many tiny files.
- Worker scaling for read/check/write/diff modes.
- Memory use when many large unified diffs are produced.

`process_files` currently collects every result, including full diff strings, before reporting. Deterministic ordering is valuable, but a huge diff run can retain substantial memory. If benchmarks justify it, spool per-file diffs or stream completed work through an ordered coordinator.

Set performance budgets only after measuring; do not complicate the parser path for hypothetical speed.

## P2: integrations and distribution

### 24. Make editor integration first-class but thin

The essential editor interface is not an LSP at first. It is:

- stdin content;
- `--stdin-filepath` for config/dialect discovery;
- formatted stdout;
- diagnostics on stderr or JSON;
- stable exit statuses;
- optional edit ranges from the library API.

Document format-on-save recipes for editors that can invoke a filter. Add range formatting only after the edit model can prove that formatting a range cannot change surrounding Markdown block interpretation.

### 25. Add repository integrations

Useful small integrations:

- A documented pre-commit configuration using `wide-md --check` or write mode.
- A minimal GitHub Action example.
- A reusable action only if version pinning and release artifacts are stable.
- `pre-commit` hooks for check and fix.
- Shell completions and a generated man page from the Clap definition.

Keep integrations as wrappers around the CLI contract; do not create behavior that exists only in one ecosystem.

### 26. Ship reproducible binaries

Before calling the tool broadly installable:

- Add CI for formatting, Clippy, tests, docs, and the supported MSRV.
- Declare `rust-version` in `Cargo.toml`.
- Test macOS, Linux, and Windows filesystem behavior.
- Publish the crate if the name and API are ready.
- Produce signed or provenance-attested release archives for common targets with SHA-256 checksums.
- Add a Homebrew formula/tap for macOS users.
- Generate an SBOM if releases become part of automated documentation pipelines.
- Include the version, target, and optionally commit in `wide-md --version`/diagnostic reports.

Do not promise a platform until its atomic replacement and metadata behavior are tested there.

### 27. Stabilize the library around edits and diagnostics

The current `format_markdown` and `unwrap_markdown` entry points return `Result<String, FormatError>`, so known unsupported syntax is explicit. A richer but still small success value would unlock structured edits, diagnostics, equivalence evidence, editors, and WASM without duplicating logic:

```rust
pub struct FormatRequest<'a> {
    pub source: &'a str,
    pub dialect: Dialect,
    pub layout: Layout,
    pub safety: SafetyPolicy,
}

pub struct FormatResult {
    pub output: String,
    pub edits: Vec<Edit>,
    pub diagnostics: Vec<Diagnostic>,
    pub evidence: EquivalenceEvidence,
}
```

Keep filesystem traversal and writing in the CLI layer. Keep parsing, edit planning, equivalence, and reports in the library. Declare semver expectations before external consumers depend on detailed event types.

A WASM build could follow naturally once the core has no filesystem assumptions, but it is a later distribution target, not a reason to weaken native safety.

## Documentation additions

### 28. State the support boundary and nonclaims

The README should say explicitly:

- Which Markdown profile is selected by default.
- Which extensions are tested and supported.
- That `--include` controls discovery only.
- That MDX and custom directives are unsupported until a matching profile exists.
- That intentional ordinary soft breaks are indistinguishable without ignore directives.
- That paragraph layout can produce very long source lines and larger line-oriented diffs.
- How config is selected for an absolute path and stdin.
- Whether a multi-file failure can leave partial writes.
- Which metadata is preserved on each supported platform.
- That semantic equivalence is checked under the selected profile, not under every Markdown renderer in existence.

Specific, bounded claims will make the tool more trustworthy than a broad “other protected structures” promise.

### 29. Add a safe adoption guide

Suggested flow:

```console
# 1. See exactly which files are discovered.
$ wide-md --list-files docs/

# 2. Inspect effective policy.
$ wide-md --show-config docs/guide.md

# 3. Preview changes.
$ wide-md --diff docs/

# 4. Check without writing in CI.
$ wide-md --check docs/

# 5. Apply intentionally.
$ wide-md docs/
```

Explain when to choose unlimited paragraph layout versus a width such as 100 or 120. Include Git diff screenshots or compact examples showing the review/blame trade-off.

### 30. Add a troubleshooting and explainability guide

Cover:

- “Why did this line join?”
- “Why did this block stay wide?”
- “Which config applied?”
- “Why was this dialect refused?”
- “How do I protect one paragraph?”
- “Why did my file metadata change?”
- “What does exit status 2 mean after some files changed?” until fail-closed batching lands.
- “How do I report a minimized formatter bug?”

An issue template should request version, platform, effective config, dialect, minimal input, actual output, expected output, and `--explain` evidence without requesting private documents.

## Smaller ideas worth keeping on the backlog

- `wide-md init` to create a commented, versioned `.wide-md.toml` after showing what it will write.
- `wide-md files` as a readable alias for discovery preview.
- `--fail-on-warning` for repositories that require every unsupported construct to be dispositioned.
- Stable diagnostic codes such as `WMD001_UNSUPPORTED_MDX_ESM` and `WMD002_SOURCE_CHANGED`.
- Per-file before/after SHA-256 in JSON reports for audit trails.
- A `--backup-suffix` mode only for non-version-controlled workflows; do not enable backup litter by default.
- Optional EditorConfig import for `max_line_length`, `end_of_line`, and indentation, but only when explicitly enabled and with provenance output.
- Repository-local baselines for warnings if real projects need gradual adoption; avoid baselining semantic mismatches.
- A changelog and config migration notes once public releases begin.
- `cargo deny` or equivalent license/advisory checks for release hygiene.
- Deterministic snapshot tests for help text, diagnostics, diff labels, and JSON schema.
- A `--diagnostic-format=github` mode only if stable file/line diagnostics make annotations useful.
- Cancellation awareness for editor use; today a broken stdout pipe is handled, but formatting work is collected before much output is written.

## Ideas I would defer or reject

- **Following symlinks by default:** the current refusal is a good safety boundary.
- **Guessing arbitrary markup from filename globs:** discovery and parsing must remain separate decisions.
- **A plugin system before dialect profiles are sound:** it would multiply unverifiable syntax boundaries.
- **Natural-language prose rewriting:** grammar, sentence style, and editorial changes are outside a source-layout formatter.
- **Automatic sentence-per-line mode soon:** language and abbreviation rules make it deceptively complex.
- **Silent fallback after semantic-verification failure:** leave bytes alone and report the mismatch.
- **Network access during formatting:** builds and editor saves should remain local, deterministic, and offline.
- **Claiming batch atomicity without a journaled recovery protocol:** two-phase preflight is valuable, but it is not a crash-safe transaction.
- **Preserving modification time after a content change by default:** that hides a real mutation from build tools and users.
- **Formatting unsupported embedded languages with regexes:** protect them with a real frontend or refuse them.

## Proposed milestones

### Milestone 0.1.1 — Honest and fail-closed

- [x] Preserve BOM-bearing Markdown and front matter.
- [x] Remove the MDX include example and refuse `.mdx` until supported.
- [x] Refuse `:::` containers before unwrapping.
- [x] Correct the README's dialect and support claims.
- [ ] Preflight all discovery/read/verification failures before any write.
- [ ] Compare source identity/content immediately before replacement.
- [x] Add regression fixtures for every confirmed corruption probe.

The confirmed single-file corruption examples are closed. The milestone remains open until a mixed-validity batch leaves every file unchanged by default and compare-before-replace protection is explicit.

### Milestone 0.2 — Inspectable trust

- Add canonical event-stream equivalence.
- Add structured edits, diagnostics, and `FormatReport`.
- Add `--show-config`, `--list-different`, `--stats`, `--explain`, and versioned JSON.
- Resolve config per target and add `--stdin-filepath`.
- Add ignore-next and off/on regions.
- Calibrate the safety tests with named mutations.

Done means every accepted change comes with inspectable evidence and every refused change explains why.

### Milestone 0.3 — Dialects and durable filesystem behavior

- Introduce named CommonMark/GFM/extended profiles.
- Replace manual inline scanning with parser-derived spans.
- Make Unicode width, tabs, BOM, and line endings explicit policies.
- Detect hard links and preserve or preflight metadata.
- Add fault-injected write tests and three-platform CI.
- Adopt official spec corpora and fuzz targets.

Done means the tool has a precise cross-platform contract for both syntax and bytes-on-disk behavior.

### Milestone 1.0 — Stable automation surface

- Freeze config, diagnostic, JSON-report, and core library schemas under semver.
- Publish supported-platform binaries, checksums/provenance, crate, and installation docs.
- Provide pre-commit, CI, shell-completion, man-page, and editor-filter recipes.
- Publish performance and compatibility boundaries.

Done means a repository can pin `wide-md`, enforce it in CI, use it in editors, and upgrade with a clear compatibility story.

## Bottom line

I would not add many formatting styles yet. I would make the existing promise exceptionally trustworthy: supported syntax is explicit, every write is race-aware and preflighted, semantic equivalence is executable, and users can see exactly what the tool decided. Once that foundation is in place, dialects, editor support, release packaging, and richer layout policies become straightforward additions rather than new sources of silent corruption.
