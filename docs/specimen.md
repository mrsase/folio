---
title: Rendering specimen
author: Folio
tags: markdown
---

# Rendering specimen

Everything the reader must render correctly, in one file.

## Inline styles

Plain text, **bold**, *italic*, ***both***, ~~struck~~, `inline code`,
a [relative link](./other.md), an [external link](https://example.com), and
autolinked <https://gpui.rs>.

## Lists

- First item
- Second item
  - Nested item
  - Another nested item
    1. Ordered child
    2. Second ordered child
- Third item

1. Ordered
2. Sequential
3. Listing

## Tasks

- [x] Render headings
- [x] Render tables
- [ ] Live reload
- [ ] Split view

## Table

| Feature    | State | Notes                    |
| ---------- | ----- | ------------------------ |
| Headings   | done  | six levels               |
| Tables     | done  | aligned columns, hover   |
| Code       | done  | syntax highlighted       |
| Task lists | done  | toggles write to disk    |

## Code

```rust
fn fib(n: u64) -> u64 {
    match n {
        0 | 1 => n,
        _ => fib(n - 1) + fib(n - 2),
    }
}
```

```python
def fib(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a
```

## Quote

> The best way to read a manual is in the tool that renders it.
>
> — Someone, probably

## Long paragraph

Markdown is a plain-text formatting syntax that can be converted to HTML. It is
often used to format readme files, for writing messages in online discussion
forums, and to create rich text using a plain text editor. The syntax is
designed to be readable as-is, without looking like it has been marked up with
tags or formatting instructions, unlike the syntax of markup languages such as
Rich Text Format or HTML.
