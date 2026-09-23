use languages::language_by_name;
use warp_editor::content::selection_model::BufferSelectionModel;
use warp_editor::content::text::IndentBehavior;
use warpui_core::App;

use super::*;
use crate::SyntaxTreeState;

fn highlighted_text(language: &str, source: &'static str, color: ColorU) -> Vec<String> {
    let language = language_by_name(language).unwrap();
    App::test((), move |mut app| async move {
        let buffer = app.add_model(|_| Buffer::new(Box::new(|_, _| IndentBehavior::Ignore)));
        let selection = app.add_model(|_| BufferSelectionModel::new(buffer.clone()));
        buffer.update(&mut app, |buffer, ctx| {
            *buffer = Buffer::from_plain_text(
                source,
                None,
                Box::new(|_, _| IndentBehavior::Ignore),
                selection,
                ctx,
            );
        });
        let snapshot = buffer.read(&app, |buffer, _| buffer.buffer_snapshot());
        let trees = SyntaxTreeState::parse_text(snapshot, None, &language)
            .await
            .unwrap();
        let other = ColorU::new(0, 0, 0, 255);
        let highlighter = HighlightQuery::new(
            &language.highlight_query,
            ColorMap {
                keyword_color: ColorU::new(10, 11, 12, 255),
                function_color: other,
                string_color: ColorU::new(7, 8, 9, 255),
                type_color: ColorU::new(4, 5, 6, 255),
                number_color: other,
                comment_color: ColorU::new(1, 2, 3, 255),
                property_color: other,
                tag_color: other,
                markdown_key_color: ColorU::new(50, 60, 70, 255),
                markdown_value_color: ColorU::new(80, 90, 100, 255),
            },
        );
        buffer.read(&app, |buffer, _| {
            highlighter
                .get_highlighted_chunks(
                    CharOffset::zero()..CharOffset::from(source.chars().count()),
                    &language.highlight_query,
                    buffer,
                    &trees.host,
                )
                .iter()
                .filter(|(_, actual)| **actual == color)
                .map(|(range, _)| {
                    source
                        .chars()
                        .skip(range.start.as_usize())
                        .take(range.end.as_usize() - range.start.as_usize())
                        .collect()
                })
                .collect()
        })
    })
}

#[test]
fn markdown_separates_keys_and_same_line_values() {
    let source = "slot:What\nlimit_line: 3\n所有者前缀: human: / agent:\nWhat:\n  下一行保持原色\n";
    assert_eq!(
        highlighted_text("markdown", source, ColorU::new(50, 60, 70, 255)),
        vec!["slot", "limit_line", "所有者前缀", "What"],
    );
    assert_eq!(
        highlighted_text("markdown", source, ColorU::new(80, 90, 100, 255)),
        vec!["What", "3", "human: / agent:"],
    );
}

#[test]
fn markdown_preserves_headings_code_and_non_fields() {
    let source = "# Heading\n- 中文: 值\n  - human: 内容\n- What:\n  - 下一行保持原色\n\n~~~yaml\ncode: unchanged\n~~~\n\n    indented: unchanged\n\n- https://example.com\n- 12:30\n";
    assert_eq!(
        highlighted_text("markdown", source, ColorU::new(80, 90, 100, 255)),
        vec!["值", "内容"],
    );
    assert_eq!(
        highlighted_text("markdown", source, ColorU::new(10, 11, 12, 255)),
        vec!["Heading"],
    );
}

#[test]
fn python_colons_keep_python_highlighting() {
    let source = "message = \"slot:What\"\n";
    assert_eq!(
        highlighted_text("python", source, ColorU::new(7, 8, 9, 255)),
        vec!["\"slot:What\""],
    );
    assert_eq!(
        highlighted_text("python", source, ColorU::new(80, 90, 100, 255)),
        Vec::<String>::new(),
    );
}

#[test]
fn markdown_preserves_times_in_prose() {
    let source = "- 不并入 22:00 软提醒——职能不同：软提醒报强杀倒计时，这条裁入睡方式\n- 22:00\n- 明天 9:05 提醒\n- 开放 19:00–23:00\n- 午夜 0:00\n- 结束 23:59\n";
    assert!(highlighted_text("markdown", source, ColorU::new(50, 60, 70, 255)).is_empty());
    assert!(highlighted_text("markdown", source, ColorU::new(80, 90, 100, 255)).is_empty());
}

#[test]
fn markdown_preserves_fields_with_time_values_and_numeric_keys() {
    let source = "- 时间: 22:00\n- version2: stable\n- version2:30\n- 编号 24:00\n- 编号 9:60\n- 编号 123:45\n";
    assert_eq!(
        highlighted_text("markdown", source, ColorU::new(50, 60, 70, 255)),
        vec![
            "时间",
            "version2",
            "version2",
            "编号 24",
            "编号 9",
            "编号 123"
        ],
    );
    assert_eq!(
        highlighted_text("markdown", source, ColorU::new(80, 90, 100, 255)),
        vec!["22:00", "stable", "30", "00", "60", "45"],
    );
}
