//! The sidebar's groups: which headings the visible sessions fall under for
//! the chosen [`SessionGrouping`], and how many sessions each is hiding.

use std::collections::HashMap;
use std::path::PathBuf;

use flint_agent::AgentKind;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::SessionGrouping;
use crate::project;
use crate::session::Bucket;

/// What a heading stands for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GroupKey {
    /// A repository (every worktree of it) and the folder within it.
    Project(PathBuf, PathBuf),
    Status(Bucket),
    Agent(AgentKind),
}

#[derive(Debug, Clone)]
pub struct SessionGroup {
    pub key: GroupKey,
    pub label: String,
    /// The full path behind a project heading.
    pub tooltip: Option<String>,
    /// Session indexes shown under the heading, most recently active first.
    pub rows: Vec<usize>,
    /// Listed sessions under the heading before search, filter and paging.
    pub total: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SessionGroups {
    pub groups: Vec<SessionGroup>,
    /// Matching sessions held back until "Show more".
    pub more: usize,
}

impl FlintApp {
    fn group_of(&self, ix: usize) -> (GroupKey, String, Option<String>) {
        let session = &self.sessions[ix];
        match self.grouping {
            SessionGrouping::Project => {
                if session.general {
                    return (
                        GroupKey::Project(session.workspace.clone(), PathBuf::new()),
                        "General agent".into(),
                        None,
                    );
                }
                let mut identity = project::identity(&session.workspace);
                let key = GroupKey::Project(identity.root.clone(), identity.subdir.clone());
                let tooltip = identity
                    .root
                    .join(&identity.subdir)
                    .to_string_lossy()
                    .to_string();
                // The heading describes the shared project, not whichever
                // worktree happens to have the most recent session.
                identity.worktree = None;
                (key, identity.label(), Some(tooltip))
            }
            SessionGrouping::Status => {
                let bucket = session.status().bucket();
                (GroupKey::Status(bucket), bucket.label().to_string(), None)
            }
            SessionGrouping::Agent => (
                GroupKey::Agent(session.agent),
                session.agent.label().to_string(),
                None,
            ),
        }
    }

    /// The visible sessions under their headings, at most `session_limit` of
    /// them (the active session and an inline rename editor are always kept).
    pub fn session_groups(&self, cx: &App) -> SessionGroups {
        let mut totals: HashMap<GroupKey, usize> = HashMap::new();
        for ix in (0..self.sessions.len()).filter(|&ix| self.is_listed(ix)) {
            *totals.entry(self.group_of(ix).0).or_default() += 1;
        }
        let mut ordered = self.visible_sessions(cx);
        ordered.sort_by_key(|&ix| std::cmp::Reverse(self.sessions[ix].touched));
        let listed = ordered.len();
        let mut position = 0;
        ordered.retain(|&ix| {
            position += 1;
            position <= self.session_limit
                || ix == self.active
                || self.renaming.as_ref().is_some_and(|(row, _)| *row == ix)
        });
        let more = listed - ordered.len();

        let mut groups: Vec<SessionGroup> = Vec::new();
        for ix in ordered {
            let (key, label, tooltip) = self.group_of(ix);
            match groups.iter_mut().find(|group| group.key == key) {
                Some(group) => group.rows.push(ix),
                None => groups.push(SessionGroup {
                    total: totals.get(&key).copied().unwrap_or(1),
                    key,
                    label,
                    tooltip,
                    rows: vec![ix],
                }),
            }
        }
        if self.grouping == SessionGrouping::Status {
            groups.sort_by_key(|group| match group.key {
                GroupKey::Status(bucket) => bucket as usize,
                _ => 0,
            });
        }
        SessionGroups { groups, more }
    }
}
