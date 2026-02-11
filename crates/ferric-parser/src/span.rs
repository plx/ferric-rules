//! Source location tracking for parse trees and error messages.

/// Opaque file identifier for multi-file source tracking.
///
/// Used to distinguish source code from different files in a multi-file
/// compilation context. The actual mapping from `FileId` to file paths
/// is maintained externally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A position in source code.
///
/// Tracks both byte offset (for slicing) and line/column (for human-readable
/// error messages). Lines and columns are 1-indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// A span of source code (start..end).
///
/// Represents a half-open range `[start, end)` in a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
