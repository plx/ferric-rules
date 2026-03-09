//! Source location tracking for parse trees and error messages.

/// Opaque file identifier for multi-file source tracking.
///
/// Used to distinguish source code from different files in a multi-file
/// compilation context. The actual mapping from `FileId` to file paths
/// is maintained externally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileId(pub u32);

/// A position in source code.
///
/// Tracks both byte offset (for slicing) and line/column (for human-readable
/// error messages). Lines and columns are 1-indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Position {
    /// Byte offset from the start of the file.
    pub offset: usize,
    /// Line number (1-indexed).
    pub line: u32,
    /// Column number (1-indexed).
    pub column: u32,
}

impl Position {
    /// Creates a position at the start of a file.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    /// Advances the position by one character.
    ///
    /// If the character is a newline, increments the line counter
    /// and resets the column to 1. Otherwise, increments the column.
    pub fn advance(&mut self, ch: char) {
        self.offset += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A span of source code (start..end).
///
/// Represents a half-open range `[start, end)` in a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    /// Starting position (inclusive).
    pub start: Position,
    /// Ending position (exclusive).
    pub end: Position,
    /// File this span refers to.
    pub file_id: FileId,
}

impl Span {
    /// Creates a new span from start and end positions.
    #[must_use]
    pub fn new(start: Position, end: Position, file_id: FileId) -> Self {
        Self {
            start,
            end,
            file_id,
        }
    }

    /// Creates a zero-width span at the given position.
    #[must_use]
    pub fn point(pos: Position, file_id: FileId) -> Self {
        Self {
            start: pos,
            end: pos,
            file_id,
        }
    }

    /// Merges two spans, creating a span that covers both.
    ///
    /// Assumes both spans are from the same file.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        debug_assert_eq!(self.file_id, other.file_id);
        let start = if self.start.offset < other.start.offset {
            self.start
        } else {
            other.start
        };
        let end = if self.end.offset > other.end.offset {
            self.end
        } else {
            other.end
        };
        Self {
            start,
            end,
            file_id: self.file_id,
        }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_position() -> impl Strategy<Value = Position> {
        (0..1000usize, 1..100u32, 1..100u32).prop_map(|(offset, line, column)| Position {
            offset,
            line,
            column,
        })
    }

    fn arb_span() -> impl Strategy<Value = Span> {
        (arb_position(), arb_position()).prop_map(|(a, b)| {
            let (start, end) = if a.offset <= b.offset { (a, b) } else { (b, a) };
            Span::new(start, end, FileId(0))
        })
    }

    proptest! {
        /// Invariant: advancing '\n' increments line by 1 and resets column to 1,
        /// regardless of the current position.
        #[test]
        fn advance_newline_increments_line(mut pos in arb_position()) {
            let before_line = pos.line;
            pos.advance('\n');
            // line must have increased by exactly 1
            prop_assert_eq!(pos.line, before_line + 1, "newline must increment line");
            // column resets to 1 after a newline
            prop_assert_eq!(pos.column, 1, "column must reset to 1 after newline");
        }

        /// Invariant: advancing any non-newline character increments column by 1.
        #[test]
        fn advance_non_newline_increments_column(mut pos in arb_position(), ch in any::<char>().prop_filter("not newline", |c| *c != '\n')) {
            let before_col = pos.column;
            pos.advance(ch);
            // column must have increased by exactly 1
            prop_assert_eq!(pos.column, before_col + 1, "non-newline must increment column by 1");
        }

        /// Postcondition: offset increases by exactly char.len_utf8() for any char.
        #[test]
        fn advance_offset_increases_by_utf8_len(mut pos in arb_position(), ch in any::<char>()) {
            let before_offset = pos.offset;
            let expected_increase = ch.len_utf8();
            pos.advance(ch);
            // offset reflects byte length, not codepoint count
            prop_assert_eq!(pos.offset, before_offset + expected_increase,
                "offset must increase by char.len_utf8()");
        }

        /// Invariant: advancing through an arbitrary string character-by-character produces
        /// consistent position state: offset == string.len(), line == 1 + newline count,
        /// and column tracks position on the last line.
        #[test]
        fn advance_through_string_consistent(s in "[ -~\n]{0,50}") {
            let mut pos = Position::new();
            for ch in s.chars() {
                pos.advance(ch);
            }
            // offset must equal the byte length of the string
            prop_assert_eq!(pos.offset, s.len(),
                "offset after advancing through string must equal string byte length");
            // line count: 1-indexed, so starts at 1 plus each newline seen
            // The string is at most 50 ASCII/newline chars, so u32 is safe here.
            #[allow(clippy::cast_possible_truncation)]
            let newline_count = s.chars().filter(|&c| c == '\n').count() as u32;
            prop_assert_eq!(pos.line, 1 + newline_count,
                "line count must be 1 + number of newlines in string");
            // column: number of characters since the last newline (or from start), 1-indexed
            #[allow(clippy::cast_possible_truncation)]
            let chars_after_last_newline =
                s.chars().rev().take_while(|&c| c != '\n').count() as u32;
            prop_assert_eq!(pos.column, 1 + chars_after_last_newline,
                "column must be 1 + chars since last newline");
        }

        /// Invariant: merged span covers both input spans — start is at most the minimum
        /// start offset and end is at least the maximum end offset.
        #[test]
        fn merge_covers_both_spans(s1 in arb_span(), s2 in arb_span()) {
            // Force same file_id so merge precondition is satisfied
            let s1 = Span::new(s1.start, s1.end, FileId(0));
            let s2 = Span::new(s2.start, s2.end, FileId(0));
            let merged = s1.merge(s2);
            let min_start = s1.start.offset.min(s2.start.offset);
            let max_end = s1.end.offset.max(s2.end.offset);
            prop_assert!(merged.start.offset <= min_start,
                "merged start must not exceed the earlier start offset");
            prop_assert!(merged.end.offset >= max_end,
                "merged end must be at least the later end offset");
        }

        /// Algebraic property: merge is commutative — order of arguments must not matter.
        #[test]
        fn merge_is_commutative(s1 in arb_span(), s2 in arb_span()) {
            let s1 = Span::new(s1.start, s1.end, FileId(0));
            let s2 = Span::new(s2.start, s2.end, FileId(0));
            let fwd = s1.merge(s2);
            let rev = s2.merge(s1);
            // start and end offsets must agree regardless of argument order
            prop_assert_eq!(fwd.start.offset, rev.start.offset,
                "merge start must be the same in both directions");
            prop_assert_eq!(fwd.end.offset, rev.end.offset,
                "merge end must be the same in both directions");
        }

        /// Algebraic property: merge is idempotent — merging a span with itself is a no-op.
        #[test]
        fn merge_identity(s in arb_span()) {
            let merged = s.merge(s);
            // start and end must be unchanged
            prop_assert_eq!(merged.start.offset, s.start.offset,
                "merging span with itself must not change start");
            prop_assert_eq!(merged.end.offset, s.end.offset,
                "merging span with itself must not change end");
        }
    }

    #[test]
    fn position_starts_at_1_1() {
        let pos = Position::new();
        assert_eq!(pos.offset, 0);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 1);
    }

    #[test]
    fn position_advance_increments_column() {
        let mut pos = Position::new();
        pos.advance('a');
        assert_eq!(pos.offset, 1);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 2);
    }

    #[test]
    fn position_advance_newline_increments_line() {
        let mut pos = Position::new();
        pos.advance('a');
        pos.advance('\n');
        assert_eq!(pos.offset, 2);
        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 1);
    }

    #[test]
    fn span_point_creates_zero_width_span() {
        let pos = Position::new();
        let file_id = FileId(0);
        let span = Span::point(pos, file_id);
        assert_eq!(span.start, span.end);
    }

    #[test]
    fn span_merge_combines_ranges() {
        let file_id = FileId(0);
        let span1 = Span::new(
            Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            Position {
                offset: 5,
                line: 1,
                column: 6,
            },
            file_id,
        );
        let span2 = Span::new(
            Position {
                offset: 10,
                line: 1,
                column: 11,
            },
            Position {
                offset: 15,
                line: 1,
                column: 16,
            },
            file_id,
        );
        let merged = span1.merge(span2);
        assert_eq!(merged.start.offset, 0);
        assert_eq!(merged.end.offset, 15);
    }
}
