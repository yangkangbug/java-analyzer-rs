//! Strict zero-based UTF-16 line/character positions for one borrowed source.

/// A zero-based line and UTF-16 code-unit column, not a display or grapheme column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utf16Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug)]
struct Line {
    start: usize,
    /// End of content, excluding the complete LF, CRLF or CR line ending.
    end: usize,
}

/// Line boundaries belonging to exactly the borrowed source text.
///
/// LF, CRLF and lone CR are line endings. Columns exclude line-ending bytes;
/// the position at the content end maps to the start of its line ending. There
/// is no position between CR and LF. A final line ending creates an empty line,
/// and even empty text has line zero. Valid EOF positions are accepted so this
/// conversion can represent half-open ranges; individual query APIs may reject
/// EOF. No conversion clamps, snaps, or crosses into another line.
///
/// The index stores line boundaries, not per-character offsets. Conversion scans
/// the selected line to count UTF-16 units. It is not an incremental line index.
///
/// ```
/// use java_engine::{LineIndex, Utf16Position};
/// let index = LineIndex::new("😀\r\nvalue");
/// assert_eq!(index.position_to_byte(Utf16Position { line: 1, character: 0 }), Some(6));
/// assert_eq!(index.position_to_byte(Utf16Position { line: 0, character: 1 }), None);
/// assert_eq!(index.byte_to_position(5), None); // Interior of CRLF.
/// ```
#[derive(Clone, Debug)]
pub struct LineIndex<'a> {
    text: &'a str,
    lines: Vec<Line>,
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut lines = Vec::new();
        let (mut start, mut byte) = (0, 0);
        let bytes = text.as_bytes();
        while byte < bytes.len() {
            match bytes[byte] {
                b'\r' | b'\n' => {
                    lines.push(Line { start, end: byte });
                    byte += if bytes[byte] == b'\r' && bytes.get(byte + 1) == Some(&b'\n') { 2 } else { 1 };
                    start = byte;
                }
                _ => byte += 1,
            }
        }
        lines.push(Line { start, end: text.len() });
        Self { text, lines }
    }

    /// Convert an exact UTF-16 position, rejecting missing lines, excess columns,
    /// surrogate-pair interiors and byte offsets that do not fit in `u32`.
    pub fn position_to_byte(&self, position: Utf16Position) -> Option<u32> {
        let line = self.lines.get(usize::try_from(position.line).ok()?)?;
        let mut character = 0_u32;
        for (relative, ch) in self.text[line.start..line.end].char_indices() {
            if character == position.character {
                return u32::try_from(line.start + relative).ok();
            }
            character = character.checked_add(ch.len_utf16() as u32)?;
            if character > position.character {
                return None;
            }
        }
        (character == position.character).then(|| u32::try_from(line.end).ok()).flatten()
    }

    /// Convert an exact UTF-8 boundary, rejecting out-of-bounds bytes, UTF-8
    /// character interiors and the interior of a CRLF line ending.
    pub fn byte_to_position(&self, byte: u32) -> Option<Utf16Position> {
        let byte = usize::try_from(byte).ok()?;
        if !self.text.is_char_boundary(byte) {
            return None;
        }
        let line_index = self.lines.partition_point(|line| line.start <= byte).checked_sub(1)?;
        let line = self.lines[line_index];
        if byte > line.end {
            return None;
        }
        Some(Utf16Position {
            line: u32::try_from(line_index).ok()?,
            character: u32::try_from(self.text[line.start..byte].encode_utf16().count()).ok()?,
        })
    }
}
