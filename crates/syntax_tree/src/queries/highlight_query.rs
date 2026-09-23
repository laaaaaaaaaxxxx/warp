use std::borrow::Cow;
use std::iter;
use std::ops::Range;

use arborium::tree_sitter::{Node, Query, QueryCursor, TextProvider, Tree};
use rangemap::RangeMap;
use streaming_iterator::StreamingIterator;
use string_offset::{ByteOffset, CharOffset};
use warp_editor::content::buffer::{Buffer, ToBufferByteOffset, ToBufferCharOffset};
use warp_editor::content::text::Bytes;
use warpui_core::color::ColorU;

/// Color mapping from parsed syntax token name to its corresponding highlighting color.
#[derive(Clone, Copy)]
pub struct ColorMap {
    pub keyword_color: ColorU,
    pub function_color: ColorU,
    pub string_color: ColorU,
    pub type_color: ColorU,
    pub number_color: ColorU,
    pub comment_color: ColorU,
    pub property_color: ColorU,
    pub tag_color: ColorU,
    pub markdown_key_color: ColorU,
    pub markdown_value_color: ColorU,
}

/// Query for retrieving syntax highlighting information on the tokens.
pub struct HighlightQuery {
    highlight_map: Vec<Option<ColorU>>,
}

impl HighlightQuery {
    pub fn new(query: &Query, color_map: ColorMap) -> Self {
        let highlight_map = query
            .capture_names()
            .iter()
            .map(|name| convert_capture_name_to_color(name, &color_map))
            .collect();

        Self { highlight_map }
    }

    /// Given the a character range, return its corresponding highlight colors.
    pub fn get_highlighted_chunks(
        &self,
        range: Range<CharOffset>,
        query: &Query,
        buffer: &Buffer,
        tree: &Tree,
    ) -> RangeMap<CharOffset, ColorU> {
        let mut range_map = RangeMap::new();

        let mut cursor = QueryCursor::new();
        let byte_start = range.start.to_buffer_byte_offset(buffer).as_usize();
        let byte_end = range.end.to_buffer_byte_offset(buffer).as_usize();
        cursor.set_byte_range(byte_start..byte_end);
        let mut captures = cursor.captures(query, tree.root_node(), TextBuffer(buffer));

        while let Some(matches) = captures.next() {
            for cap in matches.0.captures {
                let capture_name = query.capture_names()[cap.index as usize];
                let node_range = cap.node.byte_range();
                let insertion_ranges = match capture_name {
                    "comment.markdown_key" | "type.markdown_value" => {
                        let bytes: Vec<_> = TextBuffer(buffer)
                            .text(cap.node)
                            .flatten()
                            .copied()
                            .collect();
                        let text = std::str::from_utf8(&bytes).expect("inline text is UTF-8");
                        Cow::Owned(markdown_field_ranges(
                            text,
                            cap.node.start_byte(),
                            capture_name == "comment.markdown_key",
                        ))
                    }
                    _ => Cow::Borrowed(std::slice::from_ref(&node_range)),
                };
                let color = self
                    .highlight_map
                    .get(cap.index as usize)
                    .and_then(|inner| *inner);

                if let Some(color) = color {
                    for insertion_range in insertion_ranges.iter() {
                        let char_start = ByteOffset::from(insertion_range.start + 1)
                            .to_buffer_char_offset(buffer)
                            - 1;
                        let char_end = ByteOffset::from(insertion_range.end + 1)
                            .to_buffer_char_offset(buffer)
                            - 1;
                        if char_start < char_end {
                            range_map.insert(char_start..char_end, color);
                        }
                    }
                }
            }
        }

        range_map
    }
}

fn markdown_field_ranges(text: &str, start: usize, key: bool) -> Vec<Range<usize>> {
    text.split_inclusive('\n')
        .scan(start, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, line.trim_end()))
        })
        .filter_map(|(start, line)| {
            let (name, value) = line.split_once(':')?;
            let first = name.trim_start().chars().next()?;
            if !(first.is_alphabetic() || first == '_')
                || !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' ' | '\t'))
                || value.starts_with("//")
            {
                return None;
            }
            Some(if key {
                start + name.len() - name.trim_start().len()..start + name.trim_end().len()
            } else {
                start + name.len() + 1 + value.len() - value.trim_start().len()..start + line.len()
            })
        })
        .collect()
}

fn convert_capture_name_to_color(name: &str, color_map: &ColorMap) -> Option<ColorU> {
    match name {
        "comment.markdown_key" => return Some(color_map.markdown_key_color),
        "type.markdown_value" => return Some(color_map.markdown_value_color),
        "text.title" => return Some(color_map.keyword_color),
        "text.literal" => return Some(color_map.string_color),
        "text.uri" => return Some(color_map.function_color),
        "text.reference" => return Some(color_map.property_color),
        _ => {}
    }
    match name.split('.').next() {
        Some("keyword") => Some(color_map.keyword_color),
        Some("function") => Some(color_map.function_color),
        Some("string") => Some(color_map.string_color),
        Some("type") => Some(color_map.type_color),
        Some("number") => Some(color_map.number_color),
        Some("comment") => Some(color_map.comment_color),
        Some("property") => Some(color_map.property_color),
        Some("tag") => Some(color_map.tag_color),
        Some("punctuation") => Some(color_map.comment_color),
        _ => None,
    }
}

// The default tree-sitter implementation here is unsafe (since the cursor could query invalid ranges outside of content length).
// TODO(kevin): Once we migrate buffer to store ArrayStrings. We should implement the chunks API on buffer directly to avoid collecting
// into a String and then chunking them again for highlighting.
pub struct TextSlice<'a>(pub &'a [u8]);

impl TextSlice<'_> {
    fn get(&self, range: Range<usize>) -> Self {
        Self(self.0.get(range).unwrap_or_default())
    }
}

impl AsRef<[u8]> for TextSlice<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl<'a> TextProvider<TextSlice<'a>> for TextSlice<'a> {
    type I = iter::Once<TextSlice<'a>>;

    fn text(&mut self, node: Node) -> Self::I {
        iter::once(self.get(node.byte_range()))
    }
}

pub struct TextBuffer<'a>(pub &'a Buffer);

impl<'a> TextProvider<&'a [u8]> for TextBuffer<'a> {
    type I = Bytes<'a>;

    fn text(&mut self, node: Node) -> Self::I {
        let range = node.range();
        // Tree-sitter excludes the buffer's leading marker from its byte offsets.
        self.0.bytes_in_range(
            ByteOffset::from(range.start_byte + 1),
            ByteOffset::from(range.end_byte + 1),
        )
    }
}

#[cfg(test)]
#[path = "highlight_query_tests.rs"]
mod tests;
