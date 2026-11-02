//! Conversion between byte offsets and LSP positions (line and UTF-16 unit).

use tower_lsp_server::ls_types::{Position, Range};

use libmeddle::ast::Span;

/// Position of byte `offset` in `source`.
pub fn position_at(source: &str, offset: usize) -> Position {
    let offset = offset.min(source.len());
    let before = &source[..source.floor_char_boundary(offset)];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let character = before[line_start..].encode_utf16().count();
    Position::new(line as u32, character as u32)
}

/// Byte offset of `position` in `source`, clamped to the line's end.
pub fn offset_at(source: &str, position: Position) -> usize {
    let mut line_start = 0;
    for _ in 0..position.line {
        match source[line_start..].find('\n') {
            Some(index) => line_start += index + 1,
            None => return source.len(),
        }
    }
    let line = source[line_start..].split('\n').next().unwrap_or_default();
    let mut units = 0;
    for (index, character) in line.char_indices() {
        if units >= position.character as usize {
            return line_start + index;
        }
        units += character.len_utf16();
    }
    line_start + line.len()
}

pub fn range_of(source: &str, span: &Span) -> Range {
    Range::new(
        position_at(source, span.start),
        position_at(source, span.end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_multibyte_characters() {
        let source = "a😀b\nxyz";
        let offset = source.find('b').unwrap();
        let position = position_at(source, offset);
        assert_eq!(position, Position::new(0, 3));
        assert_eq!(offset_at(source, position), offset);
        assert_eq!(
            offset_at(source, Position::new(1, 2)),
            source.find('z').unwrap()
        );
    }

    #[test]
    fn past_the_end_is_clamped() {
        assert_eq!(offset_at("ab\ncd", Position::new(0, 99)), 2);
        assert_eq!(offset_at("ab", Position::new(5, 0)), 2);
    }
}
