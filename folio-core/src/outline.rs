//! Document-derived data: heading outline, frontmatter bounds, link resolution
//! and GitHub-style anchors. Pure, unit-tested.

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

/// Parse the document once and derive the table of contents. Uses the same
/// CommonMark+GFM parser family as the renderer, so headings in code fences and
/// blockquotes are classified the way the reader shows them.
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
                    outline.frontmatter =
                        yaml.position.as_ref().map(|p| p.start.offset..p.end.offset);
                }
                Node::Toml(toml) => {
                    outline.frontmatter =
                        toml.position.as_ref().map(|p| p.start.offset..p.end.offset);
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
