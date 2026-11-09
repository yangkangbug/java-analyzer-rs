use java_engine::{LineIndex, Utf16Position};

fn pos(line: u32, character: u32) -> Utf16Position {
    Utf16Position { line, character }
}

#[test]
fn utf16_positions_are_strict_and_round_trip_content_boundaries() {
    let text = "a😀中e\u{301}\t\r\nb\rc\n";
    let index = LineIndex::new(text);
    for (position, byte) in [
        (pos(0, 0), 0),
        (pos(0, 1), 1),
        (pos(0, 3), 5),
        (pos(0, 4), 8),
        (pos(0, 5), 9),
        (pos(0, 6), 11),
        (pos(0, 7), 12),
        (pos(1, 0), 14),
        (pos(1, 1), 15),
        (pos(2, 0), 16),
        (pos(2, 1), 17),
        (pos(3, 0), 18),
    ] {
        assert_eq!(index.position_to_byte(position), Some(byte), "{position:?}");
        assert_eq!(index.byte_to_position(byte), Some(position), "byte {byte}");
    }
    for position in [pos(0, 2), pos(0, 8), pos(1, 2), pos(3, 1), pos(4, 0), pos(u32::MAX, 0), pos(0, u32::MAX)] {
        assert_eq!(index.position_to_byte(position), None, "{position:?}");
    }
    for byte in [2, 3, 4, 6, 7, 10, 13, 19, u32::MAX] {
        assert_eq!(index.byte_to_position(byte), None, "byte {byte}");
    }
}

#[test]
fn empty_final_lines_and_eof_are_valid_for_range_conversion() {
    for (text, eof) in [("", pos(0, 0)), ("x", pos(0, 1)), ("\n", pos(1, 0)), ("\r", pos(1, 0)), ("\r\n", pos(1, 0))] {
        let index = LineIndex::new(text);
        assert_eq!(index.position_to_byte(pos(0, 0)), Some(0));
        assert_eq!(index.byte_to_position(0), Some(pos(0, 0)));
        assert_eq!(index.position_to_byte(eof), Some(text.len() as u32));
        assert_eq!(index.byte_to_position(text.len() as u32), Some(eof));
        assert_eq!(index.position_to_byte(pos(eof.line + 1, 0)), None);
        assert_eq!(index.byte_to_position(text.len() as u32 + 1), None);
    }
}

#[test]
fn all_mixed_newline_boundaries_have_one_unambiguous_position() {
    let text = "\r\n\n\r\r\n😀\n\n";
    let index = LineIndex::new(text);
    for byte in 0..=text.len() as u32 {
        if let Some(position) = index.byte_to_position(byte) {
            assert_eq!(index.position_to_byte(position), Some(byte));
        } else {
            assert!(byte == 1 || byte == 5 || !text.is_char_boundary(byte as usize));
        }
    }
    for (line, byte) in [0, 2, 3, 4, 6, 11, 12].into_iter().enumerate() {
        assert_eq!(index.position_to_byte(pos(line as u32, 0)), Some(byte));
    }
}

#[test]
fn short_unicode_and_newline_combinations_match_an_independent_character_walk() {
    let parts = ["", "a", "中", "😀", "\u{301}", "\r", "\n", "\r\n"];
    for first in parts {
        for second in parts {
            for third in parts {
                let text = format!("{first}{second}{third}");
                let index = LineIndex::new(&text);
                let mut expected = Vec::new();
                let mut chars = text.char_indices().peekable();
                let (mut line, mut character) = (0, 0);
                while let Some((byte, ch)) = chars.next() {
                    expected.push((pos(line, character), byte as u32));
                    if ch == '\r' || ch == '\n' {
                        if ch == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                            chars.next();
                        }
                        line += 1;
                        character = 0;
                    } else {
                        character += ch.len_utf16() as u32;
                    }
                }
                expected.push((pos(line, character), text.len() as u32));
                for byte in 0..=text.len() as u32 + 1 {
                    assert_eq!(
                        index.byte_to_position(byte),
                        expected.iter().find(|(_, b)| *b == byte).map(|(p, _)| *p),
                        "{text:?}:{byte}"
                    );
                }
                for line in 0..=line + 1 {
                    for character in 0..=text.len() as u32 + 1 {
                        let position = pos(line, character);
                        assert_eq!(
                            index.position_to_byte(position),
                            expected.iter().find(|(p, _)| *p == position).map(|(_, b)| *b),
                            "{text:?}:{position:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn utf16_line_lengths_exclude_all_line_endings_without_changing_strict_conversion() {
    let index = LineIndex::new("a😀中\r\nb\rc\n");
    for (line, length) in [4, 1, 1, 0].into_iter().enumerate() {
        assert_eq!(index.line_utf16_len(line as u32), Some(length));
        assert!(index.position_to_byte(pos(line as u32, length)).is_some());
        assert!(index.position_to_byte(pos(line as u32, length + 1)).is_none());
    }
    assert_eq!(index.line_utf16_len(4), None);
    assert_eq!(index.line_utf16_len(u32::MAX), None);
    assert_eq!(LineIndex::new("").line_utf16_len(0), Some(0));
}
