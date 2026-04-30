use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::bottom_pane::popup_consts::MAX_POPUP_ROWS;
use crate::bottom_pane::scroll_state::ScrollState;
use crate::key_hint;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::render::Insets;
use crate::render::RectExt;

use super::unified_mentions_search::MentionType;
use super::unified_mentions_search::SearchMode;
use super::unified_mentions_search::SearchResult;
use super::unified_mentions_search::Selection;

const TAG_WIDTH: usize = "Plugin".len();
const POPUP_HORIZONTAL_INSET: u16 = 2;
const CONTENT_TAG_GAP: usize = 2;
const FOOTER_SECTION_GAP: &str = "  ";
const CURRENT_DIR_PREFIX: &str = "./";
const FOOTER_INSERT_KEY: KeyCode = KeyCode::Enter;
const FOOTER_INSERT_ALTERNATE_KEY: KeyCode = KeyCode::Tab;
const FOOTER_CLOSE_KEY: KeyCode = KeyCode::Esc;
const FOOTER_PREVIOUS_MODE_KEY: KeyCode = KeyCode::Left;
const FOOTER_NEXT_MODE_KEY: KeyCode = KeyCode::Right;
const FILESYSTEM_ACCENT_COLOR: Color = Color::Cyan;
const PLUGIN_ACCENT_COLOR: Color = Color::Magenta;

pub(super) fn render_popup(
    area: Rect,
    buf: &mut Buffer,
    rows: &[SearchResult],
    state: &ScrollState,
    empty_message: &str,
    search_mode: SearchMode,
) {
    let (list_area, hint_area) = if area.height > 2 {
        let hint_area = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };
        let list_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height - 2,
        };
        (list_area, Some(hint_area))
    } else {
        (area, None)
    };

    render_rows(
        list_area.inset(Insets::tlbr(
            /*top*/ 0,
            /*left*/ POPUP_HORIZONTAL_INSET,
            /*bottom*/ 0,
            /*right*/ 0,
        )),
        buf,
        rows,
        state,
        empty_message,
    );

    if let Some(hint_area) = hint_area {
        let hint_area = Rect {
            x: hint_area.x + POPUP_HORIZONTAL_INSET,
            y: hint_area.y,
            width: hint_area.width.saturating_sub(POPUP_HORIZONTAL_INSET),
            height: hint_area.height,
        };
        render_footer(hint_area, buf, search_mode);
    }
}

fn render_rows(
    area: Rect,
    buf: &mut Buffer,
    rows: &[SearchResult],
    state: &ScrollState,
    empty_message: &str,
) {
    if area.height == 0 {
        return;
    }
    if rows.is_empty() {
        Line::from(empty_message.italic()).render(area, buf);
        return;
    }

    let visible_items = MAX_POPUP_ROWS
        .min(rows.len())
        .min(area.height.max(1) as usize);
    let mut start_idx = state.scroll_top.min(rows.len().saturating_sub(1));
    if let Some(sel) = state.selected_idx {
        if sel < start_idx {
            start_idx = sel;
        } else if visible_items > 0 {
            let bottom = start_idx + visible_items - 1;
            if sel > bottom {
                start_idx = sel + 1 - visible_items;
            }
        }
    }

    let mut cur_y = area.y;
    let primary_column_width = rows
        .iter()
        .skip(start_idx)
        .take(visible_items)
        .map(primary_text_width)
        .max()
        .unwrap_or(0);
    for (idx, row) in rows.iter().enumerate().skip(start_idx).take(visible_items) {
        if cur_y >= area.y + area.height {
            break;
        }

        let selected = Some(idx) == state.selected_idx;
        let line = build_line(row, selected, area.width as usize, primary_column_width);
        line.render(
            Rect {
                x: area.x,
                y: cur_y,
                width: area.width,
                height: 1,
            },
            buf,
        );
        cur_y = cur_y.saturating_add(1);
    }
}

fn build_line(
    row: &SearchResult,
    selected: bool,
    width: usize,
    primary_column_width: usize,
) -> Line<'static> {
    let base_style = if selected {
        Style::default().bold()
    } else {
        Style::default()
    };
    let dim_style = if selected {
        Style::default().bold()
    } else {
        Style::default().dim()
    };
    let tag = mention_type_tag(row.mention_type, base_style);
    let tag_width = tag.width();
    let content_width = width.saturating_sub(tag_width.saturating_add(CONTENT_TAG_GAP));
    let content = truncate_line_with_ellipsis_if_overflow(
        content_line(row, base_style, dim_style, primary_column_width),
        content_width,
    );
    let rendered_content_width = content.width();
    let mut spans = Vec::new();
    spans.extend(content.spans);
    let padding = width.saturating_sub(rendered_content_width.saturating_add(tag_width));
    if padding > 0 {
        spans.push(" ".repeat(padding).set_style(dim_style));
    }
    spans.push(tag);

    Line::from(spans)
}

fn mention_type_tag(mention_type: MentionType, base_style: Style) -> Span<'static> {
    let style = match mention_type {
        MentionType::Plugin => base_style.fg(PLUGIN_ACCENT_COLOR),
        MentionType::Skill => base_style.dim(),
        MentionType::File => base_style.fg(FILESYSTEM_ACCENT_COLOR),
        MentionType::Directory => base_style,
    };
    format!("{:<width$}", mention_type.label(), width = TAG_WIDTH).set_style(style)
}

fn content_line(
    row: &SearchResult,
    base_style: Style,
    dim_style: Style,
    primary_column_width: usize,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.extend(primary_spans(row, base_style));
    if let Some(secondary) = secondary_line(row, base_style, dim_style) {
        let padding = primary_column_width
            .saturating_sub(primary_text_width(row))
            .saturating_add(CONTENT_TAG_GAP);
        spans.push(" ".repeat(padding).set_style(dim_style));
        spans.extend(secondary.spans);
    }

    Line::from(spans)
}

fn primary_spans(row: &SearchResult, base_style: Style) -> Vec<Span<'static>> {
    if let Some(file_name) = file_name(row) {
        let style = if row.mention_type == MentionType::File {
            base_style.fg(FILESYSTEM_ACCENT_COLOR)
        } else {
            base_style
        };
        return styled_text_spans(file_name, style, /*match_indices*/ None);
    }

    let name_style = match row.mention_type {
        MentionType::Plugin => base_style.fg(PLUGIN_ACCENT_COLOR),
        MentionType::Skill => base_style.dim(),
        MentionType::File | MentionType::Directory => base_style,
    };
    styled_text_spans(&row.display_name, name_style, row.match_indices.as_deref())
}

fn secondary_line(
    row: &SearchResult,
    base_style: Style,
    dim_style: Style,
) -> Option<Line<'static>> {
    if file_name(row).is_some() {
        let mut spans = path_spans(row, base_style);
        if let Some(description) = row
            .description
            .as_deref()
            .filter(|description| !description.is_empty())
        {
            spans.push(FOOTER_SECTION_GAP.set_style(dim_style));
            spans.push(description.to_string().set_style(dim_style));
        }
        return Some(Line::from(spans));
    }

    row.description
        .as_deref()
        .filter(|description| !description.is_empty())
        .map(|description| Line::from(description.to_string().set_style(dim_style)))
}

fn path_spans(row: &SearchResult, base_style: Style) -> Vec<Span<'static>> {
    let file_name_start = file_name_start(row);
    let path_style = base_style.dim();
    if file_name_start == 0 {
        return styled_text_spans(CURRENT_DIR_PREFIX, path_style, /*match_indices*/ None);
    }
    if file_name_start != usize::MAX {
        let byte_start = row
            .display_name
            .char_indices()
            .nth(file_name_start)
            .map(|(idx, _)| idx)
            .unwrap_or(row.display_name.len());
        return styled_text_spans(
            &row.display_name[..byte_start],
            path_style,
            row.match_indices.as_deref(),
        );
    }

    styled_text_spans(&row.display_name, base_style, /*match_indices*/ None)
}

fn primary_text_width(row: &SearchResult) -> usize {
    file_name(row)
        .map(|file_name| file_name.chars().count())
        .unwrap_or_else(|| row.display_name.chars().count())
}

fn file_name(row: &SearchResult) -> Option<&str> {
    let file_name_start = file_name_start(row);
    if file_name_start == usize::MAX {
        return None;
    }
    if file_name_start == 0 {
        return Some(&row.display_name);
    }

    let byte_start = row
        .display_name
        .char_indices()
        .nth(file_name_start)
        .map(|(idx, _)| idx)
        .unwrap_or(row.display_name.len());
    Some(&row.display_name[byte_start..])
}

fn file_name_start(row: &SearchResult) -> usize {
    match row.selection {
        Selection::File(_) if row.mention_type.is_filesystem() => row
            .display_name
            .rfind(['/', '\\'])
            .map(|idx| row.display_name[..idx + 1].chars().count())
            .unwrap_or(0),
        Selection::File(_) | Selection::Tool { .. } => usize::MAX,
    }
}

fn styled_text_spans(
    text: &str,
    base_style: Style,
    match_indices: Option<&[usize]>,
) -> Vec<Span<'static>> {
    let Some(match_indices) = match_indices else {
        return vec![text.to_string().set_style(base_style)];
    };

    let mut spans = Vec::with_capacity(text.len());
    let mut idx_iter = match_indices.iter().peekable();
    for (char_idx, ch) in text.chars().enumerate() {
        let mut style = base_style;
        if idx_iter.peek().is_some_and(|next| **next == char_idx) {
            idx_iter.next();
            style = style.bold();
        }
        spans.push(ch.to_string().set_style(style));
    }
    spans
}

fn render_footer(area: Rect, buf: &mut Buffer, search_mode: SearchMode) {
    let right_line = search_mode_indicator_line(search_mode);
    let right_width = right_line.width() as u16;
    let gap = u16::from(right_width > 0);
    let left_width = area.width.saturating_sub(right_width).saturating_sub(gap);
    let left_line =
        truncate_line_with_ellipsis_if_overflow(footer_hint_line(), left_width as usize);
    left_line.render(
        Rect {
            x: area.x,
            y: area.y,
            width: left_width,
            height: 1,
        },
        buf,
    );
    if right_width > 0 && right_width <= area.width {
        right_line.render(
            Rect {
                x: area.x + area.width - right_width,
                y: area.y,
                width: right_width,
                height: 1,
            },
            buf,
        );
    }
}

fn footer_hint_line() -> Line<'static> {
    Line::from(vec![
        key_hint::plain(FOOTER_INSERT_KEY).into(),
        "/".dim(),
        key_hint::plain(FOOTER_INSERT_ALTERNATE_KEY).into(),
        " insert · ".dim(),
        key_hint::plain(FOOTER_CLOSE_KEY).into(),
        " close · ".dim(),
        key_hint::plain(FOOTER_PREVIOUS_MODE_KEY).into(),
        "/".dim(),
        key_hint::plain(FOOTER_NEXT_MODE_KEY).into(),
        " switch search modes".dim(),
    ])
}

fn search_mode_indicator_line(active_search_mode: SearchMode) -> Line<'static> {
    let modes = [
        SearchMode::Results,
        SearchMode::FilesystemOnly,
        SearchMode::Tools,
    ];
    let mut spans = Vec::with_capacity(modes.len() * 2 - 1);

    for (index, search_mode) in modes.into_iter().enumerate() {
        if index > 0 {
            spans.push(FOOTER_SECTION_GAP.dim());
        }

        if search_mode == active_search_mode {
            let label = format!("[{}]", search_mode.label());
            let style = match search_mode {
                SearchMode::Results | SearchMode::FilesystemOnly => {
                    Style::default().fg(FILESYSTEM_ACCENT_COLOR).bold()
                }
                SearchMode::Tools => Style::default().fg(PLUGIN_ACCENT_COLOR).bold(),
            };
            spans.push(label.set_style(style));
        } else {
            spans.push(format!(" {} ", search_mode.label()).dim());
        }
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn render_popup_text(
        query: &str,
        rows: &[SearchResult],
        state: ScrollState,
        width: u16,
        search_mode: SearchMode,
    ) -> String {
        let area = Rect::new(0, 0, width, (MAX_POPUP_ROWS as u16) + 2);
        let mut buf = Buffer::empty(area);
        render_popup(area, &mut buf, rows, &state, "no matches", search_mode);

        let popup = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!("› {query}\n\n{popup}")
    }

    fn tool_result(
        display_name: &str,
        description: Option<&str>,
        mention_type: MentionType,
    ) -> SearchResult {
        SearchResult {
            display_name: display_name.to_string(),
            description: description.map(str::to_string),
            mention_type,
            selection: Selection::Tool {
                insert_text: format!("${display_name}"),
                path: None,
            },
            match_indices: None,
            score: 0,
        }
    }

    fn file_result(path: &str, mention_type: MentionType) -> SearchResult {
        SearchResult {
            display_name: path.to_string(),
            description: None,
            mention_type,
            selection: Selection::File(PathBuf::from(path)),
            match_indices: None,
            score: 0,
        }
    }

    #[test]
    fn unified_mentions_mixed_results_snapshot() {
        let rows = vec![
            tool_result(
                "Google Calendar",
                Some("Connect calendars and event management"),
                MentionType::Plugin,
            ),
            tool_result(
                "Google Calendar",
                Some("Find availability and plan event changes"),
                MentionType::Skill,
            ),
            file_result("src/google/calendar.rs", MentionType::File),
            file_result("src/google", MentionType::Directory),
        ];
        let state = ScrollState {
            selected_idx: Some(0),
            scroll_top: 0,
        };

        insta::assert_snapshot!(
            "unified_mentions_mixed_results",
            render_popup_text(
                "@goog",
                &rows,
                state,
                /*width*/ 86,
                SearchMode::Results
            )
        );
    }

    #[test]
    fn unified_mentions_narrow_width_truncation_snapshot() {
        let rows = vec![
            tool_result(
                "Google Calendar",
                Some("Connect calendars and event management"),
                MentionType::Plugin,
            ),
            tool_result(
                "Google Calendar",
                Some("Find availability and plan event changes"),
                MentionType::Skill,
            ),
            file_result("src/google/calendar.rs", MentionType::File),
            file_result("src/google", MentionType::Directory),
        ];
        let state = ScrollState {
            selected_idx: Some(0),
            scroll_top: 0,
        };

        insta::assert_snapshot!(
            "unified_mentions_narrow_width_truncation",
            render_popup_text(
                "@goog",
                &rows,
                state,
                /*width*/ 52,
                SearchMode::Results
            )
        );
    }

    #[test]
    fn unified_mentions_plugins_mode_footer_snapshot() {
        let rows = vec![tool_result(
            "Calendar Skill",
            Some("Find availability and plan event changes"),
            MentionType::Skill,
        )];
        let state = ScrollState {
            selected_idx: Some(0),
            scroll_top: 0,
        };

        insta::assert_snapshot!(
            "unified_mentions_plugins_mode_footer",
            render_popup_text(
                "@calendar",
                &rows,
                state,
                /*width*/ 86,
                SearchMode::Tools
            )
        );
    }

    #[test]
    fn unified_mentions_tools_mode_duplicate_display_names_snapshot() {
        let rows = vec![
            tool_result(
                "Google Calendar",
                Some("Connect calendars and event management"),
                MentionType::Plugin,
            ),
            tool_result(
                "Google Calendar",
                Some("Find availability and plan event changes"),
                MentionType::Skill,
            ),
        ];
        let state = ScrollState {
            selected_idx: Some(0),
            scroll_top: 0,
        };

        insta::assert_snapshot!(
            "unified_mentions_tools_mode_duplicate_display_names",
            render_popup_text("@goog", &rows, state, /*width*/ 86, SearchMode::Tools)
        );
    }

    #[test]
    fn unified_mentions_scrolled_results_snapshot() {
        let rows = (0..(MAX_POPUP_ROWS + 2))
            .map(|idx| file_result(&format!("src/results/file_{idx:02}.rs"), MentionType::File))
            .collect::<Vec<_>>();
        let state = ScrollState {
            selected_idx: Some(MAX_POPUP_ROWS + 1),
            scroll_top: 0,
        };

        insta::assert_snapshot!(
            "unified_mentions_scrolled",
            render_popup_text(
                "@file",
                &rows,
                state,
                /*width*/ 72,
                SearchMode::Results
            )
        );
    }

    #[test]
    fn unified_mentions_filesystem_only_mode_snapshot() {
        let rows = vec![
            file_result("src/google/calendar.rs", MentionType::File),
            file_result("src/google", MentionType::Directory),
        ];
        let state = ScrollState {
            selected_idx: Some(0),
            scroll_top: 0,
        };

        insta::assert_snapshot!(
            "unified_mentions_filesystem_only_mode",
            render_popup_text(
                "@goog",
                &rows,
                state,
                /*width*/ 86,
                SearchMode::FilesystemOnly
            )
        );
    }
}
