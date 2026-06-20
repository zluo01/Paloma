use scry_core::{AppError, Connector, ProviderId};

#[derive(Default)]
pub(super) struct Model {
    pub(super) connectors: Vec<Connector>,
}

pub(super) enum Msg {
    RefreshRequested,
    ConnectorsFetched(Result<Vec<Connector>, AppError>),
    ConnectClicked(ProviderId),
    ConnectionSucceeded,
    DisconnectClicked(ProviderId),
    DisconnectConfirmed(ProviderId),
    DisconnectFinished(ProviderId, Result<(), AppError>),
    PreferenceChanged {
        id: ProviderId,
        model: String,
        effort: String,
    },
    PreferenceSaveFinished(Result<(), AppError>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    Render,
    FetchConnectors,
    ShowConnectDialog(ProviderId),
    ShowDisconnectConfirmation(ProviderId),
    DisconnectProvider(ProviderId),
    PersistPreference {
        id: ProviderId,
        model: String,
        effort: String,
    },
    Warn(String),
}

impl Model {
    /// Services page workflow:
    ///
    /// - Page creation / overlay refresh: `RefreshRequested -> FetchConnectors -> ConnectorsFetched -> Render`.
    /// - Connect button: `ConnectClicked -> ShowConnectDialog -> ConnectionSucceeded -> FetchConnectors -> ConnectorsFetched -> Render`.
    /// - Disconnect button: `DisconnectClicked -> ShowDisconnectConfirmation -> DisconnectConfirmed -> DisconnectProvider -> DisconnectFinished -> FetchConnectors -> ConnectorsFetched -> Render`.
    /// - Model picker or effort picker: `PreferenceChanged -> PersistPreference -> PreferenceSaveFinished`.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::RefreshRequested => vec![Command::FetchConnectors],
            Msg::ConnectorsFetched(result) => match result {
                Ok(connectors) => {
                    self.connectors = connectors;
                    vec![Command::Render]
                },
                Err(e) => vec![Command::Warn(format!("available_connectors failed: {e}"))],
            },
            Msg::ConnectClicked(id) => vec![Command::ShowConnectDialog(id)],
            Msg::ConnectionSucceeded => vec![Command::FetchConnectors],
            Msg::DisconnectClicked(id) => vec![Command::ShowDisconnectConfirmation(id)],
            Msg::DisconnectConfirmed(id) => vec![Command::DisconnectProvider(id)],
            Msg::DisconnectFinished(id, result) => match result {
                Ok(()) => vec![Command::FetchConnectors],
                Err(e) => vec![Command::Warn(format!("disconnect {id:?} failed: {e}"))],
            },
            Msg::PreferenceChanged { id, model, effort } => {
                vec![Command::PersistPreference { id, model, effort }]
            },
            Msg::PreferenceSaveFinished(result) => match result {
                Ok(()) => vec![],
                Err(e) => vec![
                    Command::Warn(format!("set_preferences failed: {e}")),
                    Command::FetchConnectors,
                ],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(message: &str) -> AppError {
        std::io::Error::other(message).into()
    }

    fn disconnected(id: ProviderId) -> Connector {
        Connector {
            id,
            connection: None,
        }
    }

    #[test]
    fn refresh_requests_connector_fetch() {
        let mut model = Model::default();
        assert_eq!(
            model.update(Msg::RefreshRequested),
            vec![Command::FetchConnectors]
        );
    }

    #[test]
    fn fetched_connectors_replace_model_connectors() {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectorsFetched(Ok(vec![disconnected(
            ProviderId::Codex,
        )])));

        assert_eq!(cmds, vec![Command::Render]);
        assert_eq!(model.connectors.len(), 1);
        assert!(model.connectors[0].connection.is_none());
    }

    #[test]
    fn connector_fetch_failure_only_warns() {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectorsFetched(Err(error("boom"))));

        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));
        assert!(model.connectors.is_empty());
    }

    #[test]
    fn connect_click_opens_the_dialog() {
        let mut model = Model::default();
        assert_eq!(
            model.update(Msg::ConnectClicked(ProviderId::Codex)),
            vec![Command::ShowConnectDialog(ProviderId::Codex)]
        );
    }

    #[test]
    fn successful_connection_fetches_connectors() {
        let mut model = Model::default();
        assert_eq!(
            model.update(Msg::ConnectionSucceeded),
            vec![Command::FetchConnectors]
        );
    }

    #[test]
    fn disconnect_click_asks_for_confirmation() {
        let mut model = Model::default();
        assert_eq!(
            model.update(Msg::DisconnectClicked(ProviderId::Codex)),
            vec![Command::ShowDisconnectConfirmation(ProviderId::Codex)]
        );
    }

    #[test]
    fn confirmed_disconnect_calls_backend() {
        let mut model = Model::default();
        assert_eq!(
            model.update(Msg::DisconnectConfirmed(ProviderId::Codex)),
            vec![Command::DisconnectProvider(ProviderId::Codex)]
        );
    }

    #[test]
    fn finished_disconnect_fetches_connectors() {
        let mut model = Model::default();
        assert_eq!(
            model.update(Msg::DisconnectFinished(ProviderId::Codex, Ok(()))),
            vec![Command::FetchConnectors]
        );
    }

    #[test]
    fn failed_disconnect_only_warns() {
        let mut model = Model::default();
        let cmds = model.update(Msg::DisconnectFinished(
            ProviderId::Codex,
            Err(error("nope")),
        ));
        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));
    }

    #[test]
    fn preference_change_saves_without_mutating_connector_state() {
        let mut model = Model {
            connectors: vec![disconnected(ProviderId::Codex)],
        };

        let cmds = model.update(Msg::PreferenceChanged {
            id: ProviderId::Codex,
            model: "new".into(),
            effort: "high".into(),
        });

        assert_eq!(
            cmds,
            vec![Command::PersistPreference {
                id: ProviderId::Codex,
                model: "new".into(),
                effort: "high".into(),
            }]
        );
        assert!(model.connectors[0].connection.is_none());
    }

    #[test]
    fn saved_preference_success_does_not_fetch_connectors() {
        let mut model = Model::default();
        assert_eq!(model.update(Msg::PreferenceSaveFinished(Ok(()))), vec![]);
    }

    #[test]
    fn saved_preference_failure_warns_and_fetches_connectors() {
        let mut model = Model::default();
        let cmds = model.update(Msg::PreferenceSaveFinished(Err(error("disk full"))));
        assert!(matches!(
            cmds.as_slice(),
            [Command::Warn(_), Command::FetchConnectors]
        ));
    }
}
