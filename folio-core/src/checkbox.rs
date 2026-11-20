//! GFM task checkbox detection and edits. Replacing only the marker byte keeps
//! every other byte — and therefore BOM, line endings and offsets — intact.

/// Locate the GFM checkbox marker (`[ ]`, `[x]`, `[X]`, `[-]`) on the line
/// containing `offset`. Returns the byte index of `[`.
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
/// source changed.
///
/// Both `[ ]` and `[x]` are three bytes, so offsets elsewhere stay valid. When
/// checking an empty marker, the file's existing style wins: a document that
/// uses `[X]` keeps uppercase.
pub fn set_checkbox(source: &mut String, marker: usize, checked: bool) -> bool {
    let Some(inner) = marker.checked_add(1) else {
        return false;
    };
    let bytes = source.as_bytes();
    if inner + 1 >= bytes.len() || bytes[marker] != b'[' || bytes[inner + 1] != b']' {
        return false;
    }
    let replacement = if checked {
        if uses_uppercase_markers(source) {
            b'X'
        } else {
            b'x'
        }
    } else {
        b' '
    };
    if bytes[inner] == replacement {
        return false;
    }
    // A single ASCII byte replaced by a single ASCII byte: UTF-8 boundaries are
    // untouched. `replace_range` is safe and length-preserving here.
    source.replace_range(inner..inner + 1, &(replacement as char).to_string());
    true
}

fn uses_uppercase_markers(source: &str) -> bool {
    source.as_bytes().windows(3).any(|window| window == b"[X]")
}

/// Flip the checkbox that owns `offset`, returning the new checked state.
pub fn toggle_checkbox_at(source: &mut String, offset: usize) -> Option<bool> {
    let marker = checkbox_marker_at(source, offset)?;
    let was_checked = matches!(source.as_bytes().get(marker + 1), Some(b'x') | Some(b'X'));
    set_checkbox(source, marker, !was_checked);
    Some(!was_checked)
}

/// Number of checkbox items in the document, `(done, total)`.
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
- [x] done item
- [ ] pending item
1. [ ] numbered
";

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
    fn uppercase_style_survives_a_round_trip() {
        let mut source = "- [X] first\n- [ ] second\n".to_string();
        let second = source.find("- [ ] second").unwrap();
        assert_eq!(toggle_checkbox_at(&mut source, second), Some(true));
        assert!(source.contains("- [X] second"), "file style is uppercase");

        let first = source.find("- [X] first").unwrap();
        assert_eq!(toggle_checkbox_at(&mut source, first), Some(false));
        assert!(source.contains("- [ ] first"));

        assert_eq!(toggle_checkbox_at(&mut source, first), Some(true));
        assert!(
            source.contains("- [X] first"),
            "the remaining uppercase marker keeps the style"
        );
    }

    #[test]
    fn marker_offsets_are_byte_exact() {
        let source = "éé\n- [ ] task\n";
        let marker = checkbox_marker_at(source, source.find("task").unwrap()).unwrap();
        assert_eq!(&source[marker..marker + 3], "[ ]");
    }
}
