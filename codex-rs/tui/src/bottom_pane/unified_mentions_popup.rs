use codex_file_search::FileMatch;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use crate::bottom_pane::popup_consts::MAX_POPUP_ROWS;
use crate::bottom_pane::scroll_state::ScrollState;

use super::unified_mentions_render::render_popup;
use super::unified_mentions_search::Candidate;
use super::unified_mentions_search::SearchMode;
use super::unified_mentions_search::SearchResult;
use super::unified_mentions_search::Selection;
use super::unified_mentions_search::filtered_candidates;

const POPUP_FOOTER_HEIGHT: u16 = 2;
const FILE_SEARCH_LOADING_MESSAGE: &str = "loading...";
const FILE_SEARCH_EMPTY_MESSAGE: &str = "no matches";

pub(crate) struct UnifiedMentionsPopup {
    query: String,
    file_search: FileSearchState,
    candidates: Vec<Candidate>,
    search_mode: SearchMode,
    state: ScrollState,
}

impl UnifiedMentionsPopup {
    pub(crate) fn new(candidates: Vec<Candidate>) -> Self {
        Self {
            query: String::new(),
            file_search: FileSearchState::new(),
            candidates,
            search_mode: SearchMode::Results,
            state: ScrollState::new(),
        }
    }

    pub(crate) fn set_candidates(&mut self, candidates: Vec<Candidate>) {
        self.candidates = candidates;
        self.clamp_selection();
    }

    pub(crate) fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        if query.is_empty() {
            self.file_search.set_empty_prompt();
        } else {
            self.file_search.set_query(query);
        }
        self.clamp_selection();
    }

    pub(crate) fn set_file_matches(&mut self, query: &str, matches: Vec<FileMatch>) {
        self.file_search.set_matches(query, matches);
        self.clamp_selection();
    }

    pub(crate) fn selected(&self) -> Option<Selection> {
        let rows = self.rows();
        let idx = self.state.selected_idx?;
        rows.get(idx).map(|row| row.selection.clone())
    }

    pub(crate) fn move_up(&mut self) {
        let len = self.rows().len();
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.rows().len();
        self.state.move_down_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    pub(crate) fn previous_search_mode(&mut self) {
        self.search_mode = self.search_mode.previous();
        self.clamp_selection();
    }

    pub(crate) fn next_search_mode(&mut self) {
        self.search_mode = self.search_mode.next();
        self.clamp_selection();
    }

    pub(crate) fn calculate_required_height(&self, _width: u16) -> u16 {
        (MAX_POPUP_ROWS as u16).saturating_add(POPUP_FOOTER_HEIGHT)
    }

    fn clamp_selection(&mut self) {
        let len = self.rows().len();
        self.state.clamp_selection(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    fn rows(&self) -> Vec<SearchResult> {
        filtered_candidates(
            &self.candidates,
            self.file_search.matches(),
            &self.query,
            self.search_mode,
            self.file_search.has_matches(),
        )
    }
}

impl WidgetRef for UnifiedMentionsPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        render_popup(
            area,
            buf,
            &self.rows(),
            &self.state,
            self.file_search.empty_message(),
            self.search_mode,
        );
    }
}

struct FileSearchState {
    display_query: String,
    pending_query: String,
    waiting: bool,
    matches: Vec<FileMatch>,
    state: ScrollState,
}

impl FileSearchState {
    fn new() -> Self {
        Self {
            display_query: String::new(),
            pending_query: String::new(),
            waiting: true,
            matches: Vec::new(),
            state: ScrollState::new(),
        }
    }

    fn set_query(&mut self, query: &str) {
        if query == self.pending_query {
            return;
        }

        self.pending_query.clear();
        self.pending_query.push_str(query);
        self.waiting = true;
    }

    fn set_empty_prompt(&mut self) {
        self.display_query.clear();
        self.pending_query.clear();
        self.waiting = false;
        self.matches.clear();
        self.state.reset();
    }

    fn set_matches(&mut self, query: &str, matches: Vec<FileMatch>) {
        if query != self.pending_query {
            return;
        }

        self.display_query = query.to_string();
        self.matches = matches.into_iter().take(MAX_POPUP_ROWS).collect();
        self.waiting = false;
        let len = self.matches.len();
        self.state.clamp_selection(len);
        self.state.ensure_visible(len, len.min(MAX_POPUP_ROWS));
    }

    fn matches(&self) -> &[FileMatch] {
        &self.matches
    }

    fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }

    fn empty_message(&self) -> &'static str {
        if self.waiting {
            FILE_SEARCH_LOADING_MESSAGE
        } else {
            FILE_SEARCH_EMPTY_MESSAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_file_search::MatchType;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn file_match(index: usize) -> FileMatch {
        FileMatch {
            score: index as u32,
            path: PathBuf::from(format!("src/file_{index:02}.rs")),
            match_type: MatchType::File,
            root: PathBuf::from("/tmp/repo"),
            indices: None,
        }
    }

    #[test]
    fn set_matches_keeps_only_the_first_page_of_results() {
        let mut state = FileSearchState::new();
        state.set_query("file");
        state.set_matches(
            "file",
            (0..(MAX_POPUP_ROWS + POPUP_FOOTER_HEIGHT as usize))
                .map(file_match)
                .collect(),
        );

        assert_eq!(
            state.matches,
            (0..MAX_POPUP_ROWS).map(file_match).collect::<Vec<_>>()
        );
        assert!(state.has_matches());
    }
}
