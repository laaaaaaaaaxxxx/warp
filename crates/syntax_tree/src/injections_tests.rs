use arborium::tree_sitter::{InputEdit, Point};
use languages::language_by_name;
use rangemap::RangeSet;
use warp_editor::content::selection_model::BufferSelectionModel;
use warp_editor::content::text::IndentBehavior;
use warpui_core::App;
use warpui_core::color::ColorU;

use super::*;
use crate::{ColorMap, SyntaxTreeState};

const SOURCE: &str = "# Title\n\n```python\ndef run():\n    return 1\n```\n\n- item\n  ```yaml\n  key: value\n  other: 2\n  ```\n\n```graphql\nquery { a }\n```\n";

fn color_map(keyword_color: ColorU) -> ColorMap {
    let other = ColorU::new(0, 0, 0, 255);
    ColorMap {
        keyword_color,
        function_color: other,
        string_color: ColorU::new(7, 8, 9, 255),
        type_color: other,
        number_color: other,
        comment_color: other,
        property_color: ColorU::new(20, 21, 22, 255),
        tag_color: other,
        markdown_key_color: other,
        markdown_value_color: other,
    }
}

/// Highlights `SOURCE` as Markdown, switching to `updated` after a first highlight pass.
fn highlights(initial: ColorMap, updated: Option<ColorMap>) -> Vec<(String, ColorU)> {
    highlights_after(initial, updated, |_| {})
}

/// Like [`highlights`], after applying `edit` to the parsed injection trees without reparsing.
fn highlights_after(
    initial: ColorMap,
    updated: Option<ColorMap>,
    edit: impl FnOnce(&mut [Injection]) + 'static,
) -> Vec<(String, ColorU)> {
    App::test((), move |mut app| async move {
        let buffer = app.add_model(|_| Buffer::new(Box::new(|_, _| IndentBehavior::Ignore)));
        let selection = app.add_model(|_| BufferSelectionModel::new(buffer.clone()));
        buffer.update(&mut app, |buffer, ctx| {
            *buffer = Buffer::from_plain_text(
                SOURCE,
                None,
                Box::new(|_, _| IndentBehavior::Ignore),
                selection,
                ctx,
            );
        });
        let (snapshot, version) = buffer.read(&app, |buffer, _| {
            (buffer.buffer_snapshot(), buffer.buffer_version())
        });
        let language = language_by_name("markdown").unwrap();
        let mut trees = SyntaxTreeState::parse_text(snapshot, None, &language)
            .await
            .unwrap();
        edit(&mut trees.injections);
        let state = app.add_model(|_| {
            let mut state = SyntaxTreeState::new(buffer.downgrade(), version, initial);
            state.set_language(language);
            state.syntax_tree.lock().insert(version, trees);
            state
        });

        let mut all = RangeSet::new();
        all.insert(CharOffset::zero()..CharOffset::from(SOURCE.chars().count()));
        if let Some(updated) = updated {
            state.read(&app, |state, ctx| {
                state.highlights_in_ranges(all.clone(), None, ctx).unwrap();
            });
            state.update(&mut app, |state, _| state.set_color_map(updated));
        }
        state.read(&app, |state, ctx| {
            state
                .highlights_in_ranges(all, None, ctx)
                .unwrap()
                .iter()
                .map(|(range, color)| {
                    let text = SOURCE
                        .chars()
                        .skip(range.start.as_usize())
                        .take(range.end.as_usize() - range.start.as_usize())
                        .collect();
                    (text, *color)
                })
                .collect()
        })
    })
}

fn texts_in(highlights: &[(String, ColorU)], color: ColorU) -> Vec<&str> {
    highlights
        .iter()
        .filter(|(_, actual)| *actual == color)
        .map(|(text, _)| text.as_str())
        .collect()
}

#[test]
fn fenced_code_uses_its_own_language() {
    let keyword = ColorU::new(10, 11, 12, 255);
    let highlights = highlights(color_map(keyword), None);

    assert_eq!(
        texts_in(&highlights, keyword),
        vec!["Title", "def", "return"]
    );
    // Keys of YAML indented inside a list item parse as YAML despite the list indentation.
    assert_eq!(
        texts_in(&highlights, ColorU::new(20, 21, 22, 255)),
        vec!["key", "other"],
    );
    let strings = texts_in(&highlights, ColorU::new(7, 8, 9, 255));
    assert!(strings.contains(&"value"));
    assert!(
        !strings
            .iter()
            .any(|text| text.contains("def") || text.contains("key"))
    );
    // Unsupported languages keep the host's code block highlighting.
    assert!(strings.iter().any(|text| text.contains("query { a }")));
}

#[test]
fn deleting_a_line_of_fenced_code_keeps_highlighting() {
    let keyword = ColorU::new(10, 11, 12, 255);
    let line = "other: 2\n";
    let start = SOURCE.find(line).unwrap();
    let row = SOURCE[..start].matches('\n').count();
    let column = start - SOURCE[..start].rfind('\n').unwrap() - 1;
    let highlights = highlights_after(color_map(keyword), None, move |injections| {
        let edit = InputEdit {
            start_byte: start,
            old_end_byte: start + line.len(),
            new_end_byte: start,
            start_position: Point::new(row, column),
            old_end_position: Point::new(row + 1, 0),
            new_end_position: Point::new(row, column),
        };
        for injection in injections {
            injection.tree.edit(&edit);
        }
    });

    assert_eq!(
        texts_in(&highlights, keyword),
        vec!["Title", "def", "return"]
    );
}

#[test]
fn fenced_code_follows_color_map_changes() {
    let old_keyword = ColorU::new(10, 11, 12, 255);
    let new_keyword = ColorU::new(30, 31, 32, 255);
    let highlights = highlights(color_map(old_keyword), Some(color_map(new_keyword)));

    assert!(texts_in(&highlights, old_keyword).is_empty());
    assert_eq!(
        texts_in(&highlights, new_keyword),
        vec!["Title", "def", "return"]
    );
}
