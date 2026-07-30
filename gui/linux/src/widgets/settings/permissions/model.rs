use std::collections::HashSet;

use paloma_core::{AppError, Permission};

#[derive(Default)]
pub(super) struct State {
    pub(super) permissions: Vec<Permission>,
    query: String,
    deleting: HashSet<String>,
}

pub(super) struct Section {
    pub(super) title: String,
    pub(super) permissions: Vec<Permission>,
}

pub(super) enum Msg {
    PermissionsLoaded(Result<Vec<Permission>, AppError>),
    SearchChanged(String),
    DeleteClicked(String),
    DeleteFinished(String, Result<(), AppError>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    Render,
    DeletePermission(String),
    ShowErrorDialog(String),
    LogWarning(String),
}

impl State {
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::PermissionsLoaded(result) => match result {
                Ok(permissions) => {
                    self.permissions = permissions;
                    self.deleting
                        .retain(|prefix| self.permissions.iter().any(|p| &p.prefix == prefix));
                    vec![Command::Render]
                },
                Err(e) => vec![Command::LogWarning(format!("get_permissions failed: {e}"))],
            },
            Msg::SearchChanged(query) => {
                if self.query == query {
                    vec![]
                } else {
                    self.query = query;
                    vec![Command::Render]
                }
            },
            Msg::DeleteClicked(prefix) => {
                if self.deleting.insert(prefix.clone()) {
                    vec![Command::Render, Command::DeletePermission(prefix)]
                } else {
                    vec![]
                }
            },
            Msg::DeleteFinished(prefix, result) => {
                self.deleting.remove(&prefix);
                match result {
                    Ok(()) => {
                        self.permissions.retain(|p| p.prefix != prefix);
                        vec![Command::Render]
                    },
                    Err(e) => vec![
                        Command::Render,
                        Command::ShowErrorDialog(format!(
                            "Failed to delete permission `{prefix}`: {e}"
                        )),
                    ],
                }
            },
        }
    }

    /// Permissions matching the search, grouped into sections by leading command.
    pub(super) fn visible_sections(&self) -> Vec<Section> {
        let query = self.query.trim().to_lowercase();
        let mut sections: Vec<Section> = Vec::new();
        for permission in self
            .permissions
            .iter()
            .filter(|permission| permission_matches(permission, &query))
        {
            let title = section_title(&permission.prefix);
            match sections.iter_mut().find(|section| section.title == title) {
                Some(section) => section.permissions.push(permission.clone()),
                None => sections.push(Section {
                    title,
                    permissions: vec![permission.clone()],
                }),
            }
        }
        sections.sort_by(|a, b| a.title.cmp(&b.title));
        sections
    }

    pub(super) fn has_query(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub(super) fn is_deleting(&self, prefix: &str) -> bool {
        self.deleting.contains(prefix)
    }
}

fn section_title(prefix: &str) -> String {
    prefix
        .split_whitespace()
        .next()
        .unwrap_or(prefix)
        .to_string()
}

fn permission_matches(permission: &Permission, query: &str) -> bool {
    query.is_empty()
        || permission.prefix.to_lowercase().contains(query)
        || permission_kind(permission).contains(query)
}

fn permission_kind(permission: &Permission) -> &'static str {
    if permission.with_glob {
        "glob"
    } else {
        "exact"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(message: &str) -> AppError {
        std::io::Error::other(message).into()
    }

    fn permission(prefix: &str, with_glob: bool) -> Permission {
        Permission {
            prefix: prefix.into(),
            with_glob,
            updated_at: 0,
        }
    }

    fn loaded(state: &mut State) {
        state.update(Msg::PermissionsLoaded(Ok(vec![
            permission("cargo build", true),
            permission("git status", false),
        ])));
    }

    fn visible_prefixes(state: &State) -> Vec<String> {
        state
            .visible_sections()
            .into_iter()
            .flat_map(|section| section.permissions.into_iter().map(|p| p.prefix))
            .collect()
    }

    #[test]
    fn loaded_permissions_replace_state_and_render() {
        let mut state = State::default();
        let cmds = state.update(Msg::PermissionsLoaded(Ok(vec![permission(
            "cargo build",
            true,
        )])));

        assert_eq!(cmds, vec![Command::Render]);
        assert_eq!(state.permissions.len(), 1);
        assert_eq!(state.permissions[0].prefix, "cargo build");
    }

    #[test]
    fn load_failure_only_warns() {
        let mut state = State::default();
        let cmds = state.update(Msg::PermissionsLoaded(Err(error("boom"))));
        assert!(matches!(cmds.as_slice(), [Command::LogWarning(_)]));
    }

    #[test]
    fn search_filters_by_prefix() {
        let mut state = State::default();
        loaded(&mut state);

        state.update(Msg::SearchChanged("git".into()));
        assert_eq!(visible_prefixes(&state), vec!["git status"]);
    }

    #[test]
    fn search_filters_by_kind() {
        let mut state = State::default();
        loaded(&mut state);

        state.update(Msg::SearchChanged("glob".into()));
        assert_eq!(visible_prefixes(&state), vec!["cargo build"]);
    }

    #[test]
    fn sections_sorted_alphabetically_by_command() {
        let mut state = State::default();
        state.update(Msg::PermissionsLoaded(Ok(vec![
            permission("git status", false),
            permission("cargo build", true),
        ])));

        assert_eq!(
            state
                .visible_sections()
                .iter()
                .map(|section| section.title.clone())
                .collect::<Vec<_>>(),
            vec!["cargo", "git"]
        );
    }

    #[test]
    fn sections_group_same_command_rows() {
        let mut state = State::default();
        state.update(Msg::PermissionsLoaded(Ok(vec![
            permission("git status", false),
            permission("git log", false),
        ])));

        let sections = state.visible_sections();
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0]
                .permissions
                .iter()
                .map(|p| p.prefix.clone())
                .collect::<Vec<_>>(),
            vec!["git status", "git log"]
        );
    }

    #[test]
    fn unchanged_search_does_not_render() {
        let mut state = State::default();
        state.update(Msg::SearchChanged("cargo".into()));
        assert_eq!(state.update(Msg::SearchChanged("cargo".into())), vec![]);
    }

    #[test]
    fn delete_click_marks_prefix_in_flight() {
        let mut state = State::default();
        assert_eq!(
            state.update(Msg::DeleteClicked("cargo build".into())),
            vec![
                Command::Render,
                Command::DeletePermission("cargo build".into())
            ]
        );
        assert!(state.is_deleting("cargo build"));
        assert_eq!(
            state.update(Msg::DeleteClicked("cargo build".into())),
            vec![]
        );
    }

    #[test]
    fn successful_delete_removes_permission() {
        let mut state = State::default();
        loaded(&mut state);
        state.update(Msg::DeleteClicked("cargo build".into()));

        assert_eq!(
            state.update(Msg::DeleteFinished("cargo build".into(), Ok(()))),
            vec![Command::Render]
        );
        assert!(!state.is_deleting("cargo build"));
        assert_eq!(state.permissions.len(), 1);
        assert_eq!(state.permissions[0].prefix, "git status");
    }

    #[test]
    fn failed_delete_clears_in_flight_and_shows_error() {
        let mut state = State::default();
        state.update(Msg::DeleteClicked("cargo build".into()));

        let cmds = state.update(Msg::DeleteFinished(
            "cargo build".into(),
            Err(error("busy")),
        ));

        assert!(!state.is_deleting("cargo build"));
        assert!(matches!(
            cmds.as_slice(),
            [Command::Render, Command::ShowErrorDialog(_)]
        ));
    }
}
