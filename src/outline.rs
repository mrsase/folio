//! Document-derived data: heading outline, frontmatter bounds, and GFM task
//! checkbox edits. Everything here is pure and unit-tested without GPUI.

use markdown::mdast::Node;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// Byte offset of the heading's first character in the source.
    pub offset: usize,
    /// 1-based line number.
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    pub headings: Vec<Heading>,
    /// Byte range of a leading YAML/TOML frontmatter block, when present.
    pub frontmatter: Option<Range<usize>>,
}

impl Outline {
    /// The heading whose section contains `offset`.
    pub fn section_at(&self, offset: usize) -> Option<&Heading> {
        self.headings
            .iter()
            .rev()
            .find(|heading| heading.offset <= offset)
    }
}

/// Parse the document once and derive the table of contents.
///
/// Uses the same CommonMark+GFM parser family the renderer uses, so headings
/// inside code fences or blockquotes are classified the way the reader shows
/// them rather than by a line-prefix heuristic.
pub fn outline(source: &str) -> Outline {
    let mut outline = Outline::default();
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    let Ok(ast) = markdown::to_mdast(source, &options) else {
        return outline;
    };
    if let Some(children) = ast.children() {
        for node in children {
            match node {
                Node::Yaml(yaml) => {
                    outline.frontmatter = yaml.position.as_ref().map(|p| {
                        p.start.offset..p.end.offset
                    });
                }
                Node::Toml(toml) => {
                    outline.frontmatter = toml.position.as_ref().map(|p| {
                        p.start.offset..p.end.offset
                    });
                }
                _ => {}
            }
        }
    }
    collect_headings(&ast, &mut outline.headings);
    outline
}

fn collect_headings(node: &Node, out: &mut Vec<Heading>) {
    if let Node::Heading(heading) = node {
        let Some(position) = heading.position.as_ref() else {
            return;
        };
        let text = inline_text(&heading.children);
        if !text.is_empty() {
            out.push(Heading {
                level: heading.depth,
                text,
                offset: position.start.offset,
                line: position.start.line,
            });
        }
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_headings(child, out);
        }
    }
}

/// Flatten inline children to the label a table of contents should show.
fn inline_text(children: &[Node]) -> String {
    let mut out = String::new();
    for child in children {
        push_inline_text(child, &mut out);
    }
    out.trim().to_string()
}

fn push_inline_text(node: &Node, out: &mut String) {
    match node {
        Node::Text(text) => out.push_str(&text.value),
        Node::InlineCode(code) => out.push_str(&code.value),
        Node::InlineMath(math) => out.push_str(&math.value),
        Node::Image(image) => out.push_str(&image.alt),
        Node::Html(html) => out.push_str(&html.value),
        Node::Break(_) => out.push(' '),
        Node::FootnoteReference(reference) => {
            out.push('[');
            out.push_str(&reference.identifier);
            out.push(']');
        }
        Node::MdxTextExpression(expression) => out.push_str(&expression.value),
        _ => {
            if let Some(children) = node.children() {
                for child in children {
                    push_inline_text(child, out);
                }
            }
        }
    }
}

/// Locate the GFM checkbox marker (`[ ]`, `[x]`) on the line containing `offset`.
pub fn checkbox_marker_at(source: &str, offset: usize) -> Option<usize> {
    let offset = offset.min(source.len());
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |index| offset + index);
    let line = &source[line_start..line_end];
    let bytes = line.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'[' && bytes[index + 2] == b']' {
            let inner = bytes[index + 1];
            if inner == b' ' || inner == b'x' || inner == b'X' || inner == b'-' {
                return Some(line_start + index);
            }
        }
        index += 1;
    }
    None
}

/// Set the checkbox at `marker` (the byte index of `[`), returning whether the
/// source changed. Both `[ ]` and `[x]` are three bytes, so offsets elsewhere
/// in the document stay valid.
pub fn set_checkbox(source: &mut String, marker: usize, checked: bool) -> bool {
    let Some(inner) = marker.checked_add(1) else {
        return false;
    };
    let bytes = source.as_bytes();
    if inner + 1 >= bytes.len() || bytes[marker] != b'[' || bytes[inner + 1] != b']' {
        return false;
    }
    let replacement = if checked { b'x' } else { b' ' };
    if bytes[inner] == replacement {
        return false;
    }
    // Only ASCII bytes are replaced, so the UTF-8 boundaries are untouched.
    unsafe {
        let target = source.as_mut_ptr().add(inner);
        *target = replacement;
    }
    true
}

/// Flip the checkbox that owns `offset`, returning the new checked state.
pub fn toggle_checkbox_at(source: &mut String, offset: usize) -> Option<bool> {
    let marker = checkbox_marker_at(source, offset)?;
    let was_checked = matches!(source.as_bytes().get(marker + 1), Some(b'x') | Some(b'X'));
    set_checkbox(source, marker, !was_checked);
    Some(!was_checked)
}

/// Number of checkbox items in the document, used for the reading progress chip.
pub fn count_tasks(source: &str) -> (usize, usize) {
    let mut total = 0;
    let mut done = 0;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let list_marker = trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed
                .split_once(['.', ')'])
                .is_some_and(|(digits, rest)| {
                    !digits.is_empty()
                        && digits.bytes().all(|b| b.is_ascii_digit())
                        && rest.starts_with(' ')
                });
        if !list_marker {
            continue;
        }
        if let Some(rest) = trimmed.find('[') {
            let after = &trimmed[rest..];
            if after.len() >= 3 && after.as_bytes()[2] == b']' {
                match after.as_bytes()[1] {
                    b' ' => total += 1,
                    b'x' | b'X' => {
                        total += 1;
                        done += 1;
                    }
                    b'-' => total += 1,
                    _ => {}
                }
            }
        }
    }
    (done, total)
}

/// Local link target resolution for relative links and images.
pub fn resolve_local(base_dir: &std::path::Path, href: &str) -> Option<std::path::PathBuf> {
    if href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("mailto:")
        || href.starts_with('#')
        || href.starts_with("data:")
    {
        return None;
    }
    let decoded = percent_decode(href);
    let path = std::path::Path::new(&decoded);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    })
}

fn percent_decode(input: &str) -> String {
    if !input.contains('%') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &input[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// GitHub-style heading anchor: lowercase, punctuation dropped, spaces to `-`.
///
/// Matches how `[text](#anchor)` links are written in practice, so a table of
/// contents inside a document works without any extra bookkeeping.
pub fn slugify(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
        } else if ch == ' ' || ch == '-' || ch == '_' {
            pending_dash = true;
        }
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
---
title: Sample
---

# Top

Intro.

## Setup `cargo` *fast*

```sh
# not a heading
```

### Deep

- [x] done item
- [ ] pending item
1. [ ] numbered

[link](notes/other.md) ![pic](images/a.png)
";

    #[test]
    fn outline_skips_code_fences_and_keeps_levels() {
        let derived = outline(SAMPLE);
        let labels: Vec<_> = derived
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![(1, "Top"), (2, "Setup cargo fast"), (3, "Deep")]
        );
        assert_eq!(derived.headings[0].line, 5);
        assert_eq!(&SAMPLE[derived.headings[0].offset..][..1], "#");
    }

    #[test]
    fn frontmatter_bounds_cover_the_yaml_block() {
        let derived = outline(SAMPLE);
        let range = derived.frontmatter.unwrap();
        assert_eq!(&SAMPLE[range], "---\ntitle: Sample\n---");
    }

    #[test]
    fn section_lookup_returns_enclosing_heading() {
        let derived = outline(SAMPLE);
        let deep = derived.headings.last().unwrap().offset;
        let section = derived.section_at(deep + 5).unwrap();
        assert_eq!(section.text, "Deep");
    }

    #[test]
    fn toggling_a_checkbox_edits_only_that_marker() {
        let mut source = SAMPLE.to_string();
        let pending = source.find("- [ ] pending").unwrap();
        assert_eq!(toggle_checkbox_at(&mut source, pending), Some(true));
        assert!(source.contains("- [x] pending"));
        assert!(source.contains("- [x] done item"));

        let numbered = source.find("1. [ ] numbered").unwrap();
        assert_eq!(toggle_checkbox_at(&mut source, numbered + 6), Some(true));
        assert!(source.contains("1. [x] numbered"));

        let done = source.find("- [x] done").unwrap();
        assert_eq!(toggle_checkbox_at(&mut source, done), Some(false));
        assert!(source.contains("- [ ] done item"));
    }

    #[test]
    fn toggling_outside_a_checkbox_is_a_no_op() {
        let mut source = "just a paragraph\n".to_string();
        assert_eq!(toggle_checkbox_at(&mut source, 3), None);
        assert_eq!(source, "just a paragraph\n");
    }

    #[test]
    fn task_count_reports_done_and_total() {
        assert_eq!(count_tasks(SAMPLE), (1, 3));
    }

    #[test]
    fn slugs_match_github_anchors() {
        assert_eq!(slugify("Setup `cargo` *fast*"), "setup-cargo-fast");
        assert_eq!(slugify("2. Getting Started!"), "2-getting-started");
        assert_eq!(slugify("API: v1.2 — Notes"), "api-v12-notes");
        assert_eq!(slugify("  "), "");
    }

    #[test]
    fn links_resolve_relative_to_the_document() {
        let base = std::path::Path::new("/notes");
        assert_eq!(
            resolve_local(base, "sub/a.md").unwrap(),
            std::path::PathBuf::from("/notes/sub/a.md")
        );
        assert_eq!(
            resolve_local(base, "with%20space.md").unwrap(),
            std::path::PathBuf::from("/notes/with space.md")
        );
        assert!(resolve_local(base, "https://example.com").is_none());
        assert!(resolve_local(base, "#anchor").is_none());
    }
}
