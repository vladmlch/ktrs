//! Port of ktlint-rule-engine `PositionInTextLocator.kt`. Offsets are UTF-16 units, as on the JVM.

use std::cell::OnceCell;

/// `buildPositionInTextLocator(text)`: segment `i` spans `[starts[i], starts[i + 1] - 1]`. The text is
/// kept because the table is built lazily, on the first `locate`: a run that emits no error (most
/// format runs) never scans the whole text.
pub struct PositionInTextLocator {
    text: String,
    starts: OnceCell<Box<[usize]>>,
}

impl PositionInTextLocator {
    pub fn new(text: impl Into<String>) -> PositionInTextLocator {
        PositionInTextLocator {
            text: text.into(),
            starts: OnceCell::new(),
        }
    }

    fn starts(&self) -> &[usize] {
        self.starts.get_or_init(|| {
            let mut starts = vec![0];
            let mut units = 0;
            for c in self.text.chars() {
                units += c.len_utf16();
                if c == '\n' {
                    starts.push(units);
                }
            }
            let text_length = units;
            let last = *starts.last().unwrap();
            starts.push(text_length + usize::from(last == text_length));
            starts.into_boxed_slice()
        })
    }

    /// `(line, col)`, both 1-based; `(1, 1)` for an offset past the last segment (`SegmentTree.indexOf == -1`).
    pub fn locate(&self, offset: usize) -> (usize, usize) {
        let starts = self.starts();
        let segments = &starts[..starts.len() - 1];
        if offset >= *starts.last().unwrap() {
            return (1, 1);
        }
        let line = segments.partition_point(|&start| start <= offset) - 1;
        (line + 1, offset - segments[line] + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::PositionInTextLocator;

    #[test]
    fn segments_like_the_segment_tree() {
        let l = PositionInTextLocator::new("ab\ncd");
        assert_eq!([l.locate(0), l.locate(2), l.locate(3), l.locate(4)], [(1, 1), (1, 3), (2, 1), (2, 2)]);
        assert_eq!(l.locate(5), (1, 1), "end of text without a trailing newline is outside every segment");
        let l = PositionInTextLocator::new("ab\n");
        assert_eq!(l.locate(3), (2, 1));
        let l = PositionInTextLocator::new("é\nx");
        assert_eq!(l.locate(2), (2, 1), "UTF-16 units");
    }
}
