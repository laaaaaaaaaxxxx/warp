//! Code embedded in another language, e.g. fenced code blocks in Markdown, parsed with the
//! embedded language's own grammar so it can be highlighted like a standalone file.

use std::sync::Arc;

use arborium::tree_sitter::{Node, Parser, Query, QueryCursor, Range, Tree};
use languages::{Language, language_by_name};
use streaming_iterator::StreamingIterator;
use string_offset::{ByteOffset, CharOffset};
use warp_editor::content::buffer::{Buffer, ToBufferCharOffset};

/// An embedded code region, parsed over document coordinates so its highlights need no offset
/// translation. The parsed region is the tree's included ranges.
#[derive(Clone)]
pub(crate) struct Injection {
    pub(crate) language: Arc<Language>,
    pub(crate) tree: Tree,
}

impl Injection {
    /// Character ranges of the embedded code in the buffer.
    pub(crate) fn char_ranges(
        &self,
        buffer: &Buffer,
    ) -> impl Iterator<Item = std::ops::Range<CharOffset>> {
        // Deleting a whole line of embedded code collapses its range to empty until the reparse.
        let ranges = self.tree.included_ranges().into_iter();
        ranges
            .filter(|range| range.start_byte < range.end_byte)
            .map(move |range| {
                // Tree-sitter excludes the buffer's leading marker from its byte offsets.
                let start =
                    ByteOffset::from(range.start_byte + 1).to_buffer_char_offset(buffer) - 1;
                let end = ByteOffset::from(range.end_byte + 1).to_buffer_char_offset(buffer) - 1;
                start..end
            })
    }
}

/// Parses every region the host tree's injections query matches whose language Warp supports.
/// Regions in unsupported languages are skipped and keep the host highlighting.
pub(crate) fn parse_injections(
    parser: &mut Parser,
    text: &[u8],
    host: &Tree,
    query: &Query,
) -> Vec<Injection> {
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, host.root_node(), text);
    let mut injections = Vec::new();
    while let Some(query_match) = matches.next() {
        let mut language_name = query
            .property_settings(query_match.pattern_index)
            .iter()
            .find(|property| &*property.key == "injection.language")
            .and_then(|property| property.value.as_deref());
        let mut content = None;
        for capture in query_match.captures {
            match capture_names[capture.index as usize] {
                "injection.language" => {
                    language_name =
                        Some(capture.node.utf8_text(text).expect("buffer text is UTF-8"))
                }
                "injection.content" => content = Some(capture.node),
                _ => {}
            }
        }
        let (Some(language), Some(content)) = (language_name.and_then(language_by_name), content)
        else {
            continue;
        };

        parser
            .set_language(&language.grammar)
            .expect("incompatible grammar");
        parser
            .set_included_ranges(&included_ranges(content))
            .expect("included ranges are ordered");
        let tree = parser.parse(text, None).expect("Should succeed");
        injections.push(Injection { language, tree });
    }
    // The parser is shared across parses; later ones must see the whole document again.
    parser
        .set_included_ranges(&[])
        .expect("empty ranges are valid");
    injections
}

/// The content range minus the Markdown block continuations inside it, i.e. the list-item
/// indentation in front of each line. The content's other children are code tokens and stay in.
fn included_ranges(content: Node) -> Vec<Range> {
    let mut ranges = Vec::new();
    let mut start = (content.start_byte(), content.start_position());
    let mut walker = content.walk();
    for child in content.children(&mut walker) {
        if child.kind() != "block_continuation" {
            continue;
        }
        if child.start_byte() > start.0 {
            ranges.push(Range {
                start_byte: start.0,
                end_byte: child.start_byte(),
                start_point: start.1,
                end_point: child.start_position(),
            });
        }
        start = (child.end_byte(), child.end_position());
    }
    if content.end_byte() > start.0 {
        ranges.push(Range {
            start_byte: start.0,
            end_byte: content.end_byte(),
            start_point: start.1,
            end_point: content.end_position(),
        });
    }
    ranges
}

#[cfg(test)]
#[path = "injections_tests.rs"]
mod tests;
