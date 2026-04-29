use std::path::PathBuf;

use codex_core_skills::model::SkillMetadata;
use codex_file_search::FileMatch;
use codex_file_search::MatchType;
use codex_plugin::PluginCapabilitySummary;
use codex_utils_fuzzy_match::fuzzy_match;

use crate::skills_helpers::skill_description;
use crate::skills_helpers::skill_display_name;

const MENTION_TYPE_LABEL_PLUGIN: &str = "Plugin";
const MENTION_TYPE_LABEL_SKILL: &str = "Skill";
const MENTION_TYPE_LABEL_FILE: &str = "File";
const MENTION_TYPE_LABEL_DIRECTORY: &str = "Dir";
const SEARCH_MODE_LABEL_RESULTS: &str = "All Results";
const SEARCH_MODE_LABEL_FILESYSTEM_ONLY: &str = "Filesystem Only";
const SEARCH_MODE_LABEL_PLUGINS: &str = "Plugins";
const PLUGIN_DESCRIPTION_FALLBACK: &str = "Plugin";
const PLUGIN_CAPABILITY_LABEL_SKILLS: &str = "skills";
const PLUGIN_CAPABILITY_LABEL_SINGLE_MCP_SERVER: &str = "1 MCP server";
const PLUGIN_CAPABILITY_LABEL_SINGLE_APP: &str = "1 app";

#[derive(Clone, Debug)]
pub(crate) enum Selection {
    File(PathBuf),
    Tool {
        insert_text: String,
        path: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MentionType {
    Plugin,
    Skill,
    File,
    Directory,
}

impl MentionType {
    pub(super) fn is_filesystem(self) -> bool {
        matches!(self, Self::File | Self::Directory)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Plugin => MENTION_TYPE_LABEL_PLUGIN,
            Self::Skill => MENTION_TYPE_LABEL_SKILL,
            Self::File => MENTION_TYPE_LABEL_FILE,
            Self::Directory => MENTION_TYPE_LABEL_DIRECTORY,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchMode {
    Results,
    FilesystemOnly,
    Tools,
}

impl SearchMode {
    pub(super) fn previous(self) -> Self {
        match self {
            Self::Results => Self::Tools,
            Self::FilesystemOnly => Self::Results,
            Self::Tools => Self::FilesystemOnly,
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Results => Self::FilesystemOnly,
            Self::FilesystemOnly => Self::Tools,
            Self::Tools => Self::Results,
        }
    }

    pub(super) fn accepts(self, mention_type: MentionType) -> bool {
        match self {
            Self::Results => true,
            Self::FilesystemOnly => {
                matches!(mention_type, MentionType::File | MentionType::Directory)
            }
            Self::Tools => matches!(mention_type, MentionType::Plugin | MentionType::Skill),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Results => SEARCH_MODE_LABEL_RESULTS,
            Self::FilesystemOnly => SEARCH_MODE_LABEL_FILESYSTEM_ONLY,
            // Keep the footer copy as "Plugins", even though this mode does also include skills.
            Self::Tools => SEARCH_MODE_LABEL_PLUGINS,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Candidate {
    pub(super) display_name: String,
    pub(super) description: Option<String>,
    pub(super) search_terms: Vec<String>,
    pub(super) mention_type: MentionType,
    pub(super) selection: Selection,
}

#[derive(Clone, Debug)]
pub(super) struct SearchResult {
    pub(super) display_name: String,
    pub(super) description: Option<String>,
    pub(super) mention_type: MentionType,
    pub(super) selection: Selection,
    pub(super) match_indices: Option<Vec<usize>>,
    pub(super) score: i32,
}

impl Candidate {
    pub(super) fn to_result(&self, match_indices: Option<Vec<usize>>, score: i32) -> SearchResult {
        SearchResult {
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            mention_type: self.mention_type,
            selection: self.selection.clone(),
            match_indices,
            score,
        }
    }
}

// Unified `@` intentionally excludes app connectors for Codex App parity; `$` mentions still surface apps/connectors.
pub(crate) fn build_search_catalog(
    skills: Option<&[SkillMetadata]>,
    plugins: Option<&[PluginCapabilitySummary]>,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    if let Some(skills) = skills {
        candidates.extend(skills.iter().map(skill_candidate));
    }

    if let Some(plugins) = plugins {
        candidates.extend(plugins.iter().map(plugin_candidate));
    }

    candidates
}

pub(super) fn filtered_candidates(
    candidates: &[Candidate],
    file_matches: &[FileMatch],
    query: &str,
    search_mode: SearchMode,
    show_file_matches: bool,
) -> Vec<SearchResult> {
    let filter = query.trim();
    let mut out = Vec::new();

    for candidate in candidates {
        if !search_mode.accepts(candidate.mention_type) {
            continue;
        }
        if filter.is_empty() {
            out.push(candidate.to_result(/*match_indices*/ None, /*score*/ 0));
            continue;
        }

        if let Some((indices, score)) = best_tool_match(candidate, filter) {
            out.push(candidate.to_result(indices, score));
        }
    }

    if show_file_matches {
        out.extend(
            file_matches
                .iter()
                .map(file_match_to_row)
                .filter(|candidate| search_mode.accepts(candidate.mention_type)),
        );
    }

    sort_rows(&mut out, filter);
    out
}

fn skill_candidate(skill: &SkillMetadata) -> Candidate {
    let display_name = skill_display_name(skill);
    let description = optional_skill_description(skill);
    let skill_name = skill.name.clone();
    let search_terms = if display_name == skill.name {
        vec![skill_name.clone()]
    } else {
        vec![skill_name.clone(), display_name.clone()]
    };
    Candidate {
        display_name,
        description,
        search_terms,
        mention_type: MentionType::Skill,
        selection: Selection::Tool {
            insert_text: format!("${skill_name}"),
            path: Some(skill.path_to_skills_md.to_string_lossy().into_owned()),
        },
    }
}

fn plugin_candidate(plugin: &PluginCapabilitySummary) -> Candidate {
    let (plugin_name, marketplace_name) = plugin
        .config_name
        .split_once('@')
        .unwrap_or((plugin.config_name.as_str(), ""));
    let mut search_terms = vec![plugin_name.to_string(), plugin.config_name.clone()];
    if plugin.display_name != plugin_name {
        search_terms.push(plugin.display_name.clone());
    }
    if !marketplace_name.is_empty() {
        search_terms.push(marketplace_name.to_string());
    }

    Candidate {
        display_name: plugin.display_name.clone(),
        description: plugin_description(plugin),
        search_terms,
        mention_type: MentionType::Plugin,
        selection: Selection::Tool {
            insert_text: format!("${plugin_name}"),
            path: Some(format!("plugin://{}", plugin.config_name)),
        },
    }
}

fn plugin_description(plugin: &PluginCapabilitySummary) -> Option<String> {
    let capability_labels = plugin_capability_labels(plugin);
    plugin.description.clone().or_else(|| {
        Some(if capability_labels.is_empty() {
            PLUGIN_DESCRIPTION_FALLBACK.to_string()
        } else {
            format!(
                "{PLUGIN_DESCRIPTION_FALLBACK} - {}",
                capability_labels.join(" - ")
            )
        })
    })
}

fn plugin_capability_labels(plugin: &PluginCapabilitySummary) -> Vec<String> {
    let mut labels = Vec::new();
    if plugin.has_skills {
        labels.push(PLUGIN_CAPABILITY_LABEL_SKILLS.to_string());
    }
    if !plugin.mcp_server_names.is_empty() {
        let mcp_server_count = plugin.mcp_server_names.len();
        labels.push(if mcp_server_count == 1 {
            PLUGIN_CAPABILITY_LABEL_SINGLE_MCP_SERVER.to_string()
        } else {
            format!("{mcp_server_count} MCP servers")
        });
    }
    if !plugin.app_connector_ids.is_empty() {
        let app_count = plugin.app_connector_ids.len();
        labels.push(if app_count == 1 {
            PLUGIN_CAPABILITY_LABEL_SINGLE_APP.to_string()
        } else {
            format!("{app_count} apps")
        });
    }
    labels
}

fn optional_skill_description(skill: &SkillMetadata) -> Option<String> {
    let description = skill_description(skill).trim();
    (!description.is_empty()).then(|| description.to_string())
}

fn best_tool_match(candidate: &Candidate, filter: &str) -> Option<(Option<Vec<usize>>, i32)> {
    if let Some((indices, score)) = fuzzy_match(&candidate.display_name, filter) {
        return Some((Some(indices), score));
    }

    candidate
        .search_terms
        .iter()
        .filter(|term| *term != &candidate.display_name)
        .filter_map(|term| fuzzy_match(term, filter).map(|(_indices, score)| score))
        .min()
        .map(|score| (None, score))
}

fn sort_rows(rows: &mut [SearchResult], filter: &str) {
    let type_order = |mention_type: MentionType| match mention_type {
        MentionType::Plugin => 0,
        MentionType::Skill => 1,
        MentionType::File | MentionType::Directory => 2,
    };

    rows.sort_by(|a, b| {
        type_order(a.mention_type)
            .cmp(&type_order(b.mention_type))
            .then_with(|| compare_within_rank(a, b, filter))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
}

fn compare_within_rank(a: &SearchResult, b: &SearchResult, filter: &str) -> std::cmp::Ordering {
    if a.mention_type.is_filesystem() && b.mention_type.is_filesystem() {
        return b.score.cmp(&a.score);
    }
    if filter.is_empty() {
        return a.display_name.cmp(&b.display_name);
    }

    a.match_indices
        .is_none()
        .cmp(&b.match_indices.is_none())
        .then_with(|| a.score.cmp(&b.score))
}

fn file_match_to_row(file_match: &FileMatch) -> SearchResult {
    let mention_type = match file_match.match_type {
        MatchType::File => MentionType::File,
        MatchType::Directory => MentionType::Directory,
    };
    SearchResult {
        display_name: file_match.path.to_string_lossy().to_string(),
        description: None,
        mention_type,
        selection: Selection::File(file_match.path.clone()),
        match_indices: file_match
            .indices
            .as_ref()
            .map(|indices| indices.iter().map(|idx| *idx as usize).collect()),
        score: file_match.score as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn tool_candidate(display_name: &str, mention_type: MentionType) -> Candidate {
        Candidate {
            display_name: display_name.to_string(),
            description: None,
            search_terms: vec![display_name.to_string()],
            mention_type,
            selection: Selection::Tool {
                insert_text: format!("${display_name}"),
                path: None,
            },
        }
    }

    fn file_match(path: &str, match_type: MatchType, score: u32) -> FileMatch {
        FileMatch {
            score,
            path: PathBuf::from(path),
            match_type,
            root: PathBuf::from("/tmp/repo"),
            indices: None,
        }
    }

    #[test]
    fn unified_mentions_all_results_order_tools_before_files_and_files_by_descending_score() {
        let candidates = vec![
            tool_candidate("Bravo Plugin", MentionType::Plugin),
            tool_candidate("Alpha Skill", MentionType::Skill),
        ];
        let file_matches = vec![
            file_match("src/alpha.rs", MatchType::File, /*score*/ 1),
            file_match("src/bin", MatchType::Directory, /*score*/ 7),
            file_match("src/zeta.rs", MatchType::File, /*score*/ 9),
        ];

        let rows = filtered_candidates(
            &candidates,
            &file_matches,
            "",
            SearchMode::Results,
            /*show_file_matches*/ true,
        );

        let ordered_rows: Vec<(MentionType, String)> = rows
            .into_iter()
            .map(|row| (row.mention_type, row.display_name))
            .collect();
        assert_eq!(
            ordered_rows,
            vec![
                (MentionType::Plugin, "Bravo Plugin".to_string()),
                (MentionType::Skill, "Alpha Skill".to_string()),
                (MentionType::File, "src/zeta.rs".to_string()),
                (MentionType::Directory, "src/bin".to_string()),
                (MentionType::File, "src/alpha.rs".to_string()),
            ]
        );
    }

    #[test]
    fn unified_mentions_search_modes_filter_results_to_their_expected_types() {
        let candidates = vec![
            tool_candidate("Bravo Plugin", MentionType::Plugin),
            tool_candidate("Alpha Skill", MentionType::Skill),
        ];
        let file_matches = vec![
            file_match("src/main.rs", MatchType::File, /*score*/ 5),
            file_match("src/bin", MatchType::Directory, /*score*/ 4),
        ];

        let all_results: Vec<MentionType> = filtered_candidates(
            &candidates,
            &file_matches,
            "",
            SearchMode::Results,
            /*show_file_matches*/ true,
        )
        .into_iter()
        .map(|row| row.mention_type)
        .collect();
        assert_eq!(
            all_results,
            vec![
                MentionType::Plugin,
                MentionType::Skill,
                MentionType::File,
                MentionType::Directory,
            ]
        );

        let filesystem_only: Vec<MentionType> = filtered_candidates(
            &candidates,
            &file_matches,
            "",
            SearchMode::FilesystemOnly,
            /*show_file_matches*/ true,
        )
        .into_iter()
        .map(|row| row.mention_type)
        .collect();
        assert_eq!(
            filesystem_only,
            vec![MentionType::File, MentionType::Directory]
        );

        let tools_only: Vec<MentionType> = filtered_candidates(
            &candidates,
            &file_matches,
            "",
            SearchMode::Tools,
            /*show_file_matches*/ true,
        )
        .into_iter()
        .map(|row| row.mention_type)
        .collect();
        assert_eq!(tools_only, vec![MentionType::Plugin, MentionType::Skill]);
    }
}
