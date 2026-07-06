use scry_core::{AppError, Connection, Connector, ProviderId};

#[derive(Default)]
pub(super) struct Model {
    pub(super) connectors: Vec<Connector>,
    pub(super) connecting: Option<ProviderId>,
}

pub(super) enum Msg {
    ConnectorsFetched(Result<Vec<Connector>, AppError>),
    DisconnectClicked(ProviderId),
    DisconnectConfirmed(ProviderId),
    DisconnectFinished(ProviderId, Result<(), AppError>),
    PreferenceChanged {
        id: ProviderId,
        model: String,
        effort: String,
    },
    PreferenceSaveFinished(Result<(), AppError>),
    // dialog
    ConnectClicked(ProviderId),
    InitFinished(ProviderId, Result<Connection, AppError>),
    ConnectionSubmitted {
        provider_id: ProviderId,
        connection: Connection,
    },
    FinalizeFinished(ProviderId, Result<(), AppError>),
    CloseDialogClicked,
    DialogClosed,
}

pub(super) enum Command {
    Render,
    FetchConnectors,
    ShowDisconnectConfirmation(ProviderId),
    DisconnectProvider(ProviderId),
    PersistPreference {
        id: ProviderId,
        model: String,
        effort: String,
    },
    Warn(String),
    ShowErrorDialog(String),
    // dialog
    InitConnection(ProviderId),
    ShowConnectionDialog(ProviderId),
    ShowLoading,
    ShowChallenge {
        verification_uri: &'static str,
        user_code: String,
    },
    ShowManualInput {
        provider_id: ProviderId,
        instructions_url: Option<String>,
    },
    ShowOauth {
        provider_id: ProviderId,
        authorization_url: String,
    },
    ShowError(String),
    ShowSuccess,
    FinalizeConnection {
        provider_id: ProviderId,
        connection: Connection,
    },
    CloseConnectionDialog,
    DropConnectionDialog,
}

impl Model {
    /// Services page workflow:
    ///
    /// - Page refresh: `ConnectorsFetched -> Render`.
    /// - Disconnect button: `DisconnectClicked -> ShowDisconnectConfirmation
    ///   -> DisconnectConfirmed -> DisconnectProvider -> DisconnectFinished
    ///   -> FetchConnectors -> ConnectorsFetched -> Render`.
    /// - Model picker or effort picker: `PreferenceChanged ->
    ///   PersistPreference -> PreferenceSaveFinished`.
    /// - Connect button: `ConnectClicked -> ShowConnectionDialog + InitConnection` -> `InitFinished`
    ///   then takes exactly one of:
    ///   - device code: `ShowChallenge + FinalizeConnection` (finalize waits
    ///     for browser approval while the code stays visible),
    ///   - manual key: `ShowManualInput`, whose Connect button submits
    ///     `ConnectionSubmitted -> ShowLoading + FinalizeConnection`,
    ///   - browser sign-in: `ShowOauth`, whose Connect button submits the
    ///     pasted code the same way,
    ///   - failure: `ShowError`.
    ///
    ///   `FinalizeFinished` is `Ok -> ShowSuccess + FetchConnectors` (the
    ///   dialog auto-closes) or `Err -> ShowError`.
    /// - Closing the dialog on any path: `DialogClosed -> DropConnectionDialog`.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::ConnectorsFetched(result) => match result {
                Ok(connectors) => {
                    self.connectors = connectors;
                    vec![Command::Render]
                },
                Err(e) => vec![Command::Warn(format!("available_connectors failed: {e}"))],
            },
            Msg::DisconnectClicked(id) => vec![Command::ShowDisconnectConfirmation(id)],
            Msg::DisconnectConfirmed(id) => vec![Command::DisconnectProvider(id)],
            Msg::DisconnectFinished(id, result) => match result {
                Ok(()) => vec![Command::FetchConnectors],
                Err(e) => vec![Command::ShowErrorDialog(format!(
                    "Disconnecting {id} failed: {e}"
                ))],
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
            Msg::ConnectClicked(id) => {
                if self.connecting.is_some() {
                    return vec![];
                }
                self.connecting = Some(id);
                vec![
                    Command::ShowConnectionDialog(id),
                    Command::InitConnection(id),
                ]
            },
            Msg::InitFinished(provider_id, result) => {
                if self.connecting != Some(provider_id) {
                    return match result {
                        Ok(_) => vec![],
                        Err(e) => vec![Command::Warn(format!(
                            "stale connect failure for {provider_id}: {e}"
                        ))],
                    };
                }
                match result {
                    Ok(Connection::DeviceCode {
                        verification_uri,
                        user_code,
                        transaction_payload,
                    }) => vec![
                        Command::ShowChallenge {
                            verification_uri,
                            user_code: user_code.clone(),
                        },
                        Command::FinalizeConnection {
                            provider_id,
                            connection: Connection::DeviceCode {
                                verification_uri,
                                user_code,
                                transaction_payload,
                            },
                        },
                    ],
                    Ok(Connection::ManualInput {
                        instructions_url, ..
                    }) => vec![Command::ShowManualInput {
                        provider_id,
                        instructions_url,
                    }],
                    Ok(Connection::BrowserRedirect { authorization_url }) => {
                        vec![Command::ShowOauth {
                            provider_id,
                            authorization_url,
                        }]
                    },
                    Err(e) => vec![Command::ShowError(e.to_string())],
                }
            },
            Msg::ConnectionSubmitted {
                provider_id,
                connection,
            } => {
                if self.connecting != Some(provider_id) {
                    return vec![];
                }
                vec![
                    Command::ShowLoading,
                    Command::FinalizeConnection {
                        provider_id,
                        connection,
                    },
                ]
            },
            Msg::FinalizeFinished(provider_id, result) => {
                if self.connecting == Some(provider_id) {
                    match result {
                        Ok(()) => vec![Command::ShowSuccess, Command::FetchConnectors],
                        Err(e) => vec![Command::ShowError(e.to_string())],
                    }
                } else {
                    match result {
                        // The backend finished after the dialog moved on; the
                        // connection exists either way, so still refresh.
                        Ok(()) => vec![Command::FetchConnectors],
                        Err(e) => vec![Command::Warn(format!(
                            "stale connect failure for {provider_id}: {e}"
                        ))],
                    }
                }
            },
            Msg::CloseDialogClicked => vec![Command::CloseConnectionDialog],
            Msg::DialogClosed => {
                self.connecting = None;
                vec![Command::DropConnectionDialog]
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

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

    fn device_code() -> Connection {
        Connection::DeviceCode {
            verification_uri: "https://example.com/device",
            user_code: "ABCD-1234".into(),
            transaction_payload: Value::Null,
        }
    }

    fn manual_input(instructions_url: Option<&str>) -> Connection {
        Connection::ManualInput {
            api_key: String::new(),
            instructions_url: instructions_url.map(str::to_string),
        }
    }

    /// Walk the model to the state where `id`'s connection dialog is open.
    fn connecting(id: ProviderId) -> Model {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectClicked(id));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowConnectionDialog(_), Command::InitConnection(_)]
        ));
        model
    }

    #[test]
    fn refresh_workflow_renders_fetched_connectors() {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectorsFetched(Ok(vec![disconnected(
            ProviderId::Codex,
        )])));

        assert!(matches!(cmds.as_slice(), [Command::Render]));
        assert_eq!(model.connectors.len(), 1);
        assert!(model.connectors[0].connection.is_none());
    }

    #[test]
    fn refresh_workflow_failure_only_warns() {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectorsFetched(Err(error("boom"))));

        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));
        assert!(model.connectors.is_empty());
    }

    #[test]
    fn disconnect_workflow_confirms_disconnects_and_refreshes() {
        let mut model = Model::default();

        let cmds = model.update(Msg::DisconnectClicked(ProviderId::Codex));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowDisconnectConfirmation(ProviderId::Codex)]
        ));

        let cmds = model.update(Msg::DisconnectConfirmed(ProviderId::Codex));
        assert!(matches!(
            cmds.as_slice(),
            [Command::DisconnectProvider(ProviderId::Codex)]
        ));

        let cmds = model.update(Msg::DisconnectFinished(ProviderId::Codex, Ok(())));
        assert!(matches!(cmds.as_slice(), [Command::FetchConnectors]));

        let cmds = model.update(Msg::ConnectorsFetched(Ok(vec![])));
        assert!(matches!(cmds.as_slice(), [Command::Render]));
    }

    #[test]
    fn disconnect_workflow_failure_shows_an_error_dialog() {
        let mut model = Model::default();
        let _ = model.update(Msg::DisconnectClicked(ProviderId::Codex));
        let _ = model.update(Msg::DisconnectConfirmed(ProviderId::Codex));

        let cmds = model.update(Msg::DisconnectFinished(
            ProviderId::Codex,
            Err(error("nope")),
        ));
        assert!(matches!(cmds.as_slice(), [Command::ShowErrorDialog(_)]));
    }

    #[test]
    fn preference_workflow_persists_silently() {
        let mut model = Model {
            connectors: vec![disconnected(ProviderId::Codex)],
            ..Model::default()
        };

        let cmds = model.update(Msg::PreferenceChanged {
            id: ProviderId::Codex,
            model: "new".into(),
            effort: "high".into(),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::PersistPreference {
                id: ProviderId::Codex,
                model,
                effort,
            }] if model == "new" && effort == "high"
        ));
        assert!(model.connectors[0].connection.is_none());

        assert!(model.update(Msg::PreferenceSaveFinished(Ok(()))).is_empty());
    }

    #[test]
    fn preference_workflow_failure_warns_and_reloads() {
        let mut model = Model::default();
        let _ = model.update(Msg::PreferenceChanged {
            id: ProviderId::Codex,
            model: "new".into(),
            effort: "high".into(),
        });

        let cmds = model.update(Msg::PreferenceSaveFinished(Err(error("disk full"))));
        assert!(matches!(
            cmds.as_slice(),
            [Command::Warn(_), Command::FetchConnectors]
        ));
    }

    #[test]
    fn connect_click_opens_the_dialog_and_inits_connection() {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectClicked(ProviderId::Codex));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowConnectionDialog(ProviderId::Codex),
                Command::InitConnection(ProviderId::Codex)
            ]
        ));
        assert_eq!(model.connecting, Some(ProviderId::Codex));
    }

    #[test]
    fn connect_click_is_ignored_while_a_dialog_is_open() {
        let mut model = connecting(ProviderId::Codex);
        assert!(
            model
                .update(Msg::ConnectClicked(ProviderId::OpenAI))
                .is_empty()
        );
        assert_eq!(model.connecting, Some(ProviderId::Codex));
    }

    #[test]
    fn device_code_workflow_shows_challenge_finalizes_and_closes() {
        let mut model = connecting(ProviderId::Codex);

        // Init returned a device-code challenge: the code is shown and the
        // finalize wait starts immediately.
        let cmds = model.update(Msg::InitFinished(ProviderId::Codex, Ok(device_code())));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowChallenge { user_code, .. },
                Command::FinalizeConnection {
                    provider_id: ProviderId::Codex,
                    connection: Connection::DeviceCode { .. },
                }
            ] if user_code == "ABCD-1234"
        ));

        // Browser approval resolves the finalize.
        let cmds = model.update(Msg::FinalizeFinished(ProviderId::Codex, Ok(())));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowSuccess, Command::FetchConnectors]
        ));

        // The auto-close reports back and frees the flow.
        let cmds = model.update(Msg::DialogClosed);
        assert!(matches!(cmds.as_slice(), [Command::DropConnectionDialog]));
        assert!(model.connecting.is_none());
    }

    #[test]
    fn manual_key_workflow_submits_and_succeeds() {
        let mut model = connecting(ProviderId::Anthropic);

        let cmds = model.update(Msg::InitFinished(
            ProviderId::Anthropic,
            Ok(manual_input(Some("https://example.com/keys"))),
        ));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowManualInput {
                provider_id: ProviderId::Anthropic,
                instructions_url: Some(_),
            }]
        ));

        let cmds = model.update(Msg::ConnectionSubmitted {
            provider_id: ProviderId::Anthropic,
            connection: Connection::ManualInput {
                api_key: "key".into(),
                instructions_url: None,
            },
        });
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowLoading,
                Command::FinalizeConnection {
                    provider_id: ProviderId::Anthropic,
                    ..
                }
            ]
        ));

        let cmds = model.update(Msg::FinalizeFinished(ProviderId::Anthropic, Ok(())));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowSuccess, Command::FetchConnectors]
        ));

        let cmds = model.update(Msg::DialogClosed);
        assert!(matches!(cmds.as_slice(), [Command::DropConnectionDialog]));
        assert!(model.connecting.is_none());
    }

    #[test]
    fn oauth_workflow_submits_the_pasted_code() {
        let mut model = connecting(ProviderId::Anthropic);

        let cmds = model.update(Msg::InitFinished(
            ProviderId::Anthropic,
            Ok(Connection::BrowserRedirect {
                authorization_url: "https://example.com/auth".into(),
            }),
        ));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowOauth {
                provider_id: ProviderId::Anthropic,
                ..
            }]
        ));

        let cmds = model.update(Msg::ConnectionSubmitted {
            provider_id: ProviderId::Anthropic,
            connection: Connection::BrowserRedirect {
                authorization_url: "pasted-code".into(),
            },
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowLoading, Command::FinalizeConnection { .. }]
        ));

        let cmds = model.update(Msg::FinalizeFinished(ProviderId::Anthropic, Ok(())));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowSuccess, Command::FetchConnectors]
        ));
    }

    #[test]
    fn failed_init_shows_the_error_page_until_closed() {
        let mut model = connecting(ProviderId::Codex);

        let cmds = model.update(Msg::InitFinished(ProviderId::Codex, Err(error("offline"))));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowError(message)] if message == "offline"
        ));

        // Close on the error page requests the close; the closed dialog then
        // reports back and frees the flow.
        let cmds = model.update(Msg::CloseDialogClicked);
        assert!(matches!(cmds.as_slice(), [Command::CloseConnectionDialog]));

        let cmds = model.update(Msg::DialogClosed);
        assert!(matches!(cmds.as_slice(), [Command::DropConnectionDialog]));
        assert!(model.connecting.is_none());
    }

    #[test]
    fn failed_finalize_shows_the_error_page() {
        let mut model = connecting(ProviderId::Anthropic);
        let _ = model.update(Msg::InitFinished(
            ProviderId::Anthropic,
            Ok(manual_input(None)),
        ));
        let _ = model.update(Msg::ConnectionSubmitted {
            provider_id: ProviderId::Anthropic,
            connection: Connection::ManualInput {
                api_key: "key".into(),
                instructions_url: None,
            },
        });

        let cmds = model.update(Msg::FinalizeFinished(
            ProviderId::Anthropic,
            Err(error("bad key")),
        ));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowError(message)] if message == "bad key"
        ));
    }

    #[test]
    fn abandoned_flow_drops_stale_init_results_and_submits() {
        let mut model = connecting(ProviderId::Codex);
        // Esc while still loading.
        let _ = model.update(Msg::DialogClosed);

        assert!(
            model
                .update(Msg::InitFinished(ProviderId::Codex, Ok(device_code())))
                .is_empty()
        );
        assert!(
            model
                .update(Msg::ConnectionSubmitted {
                    provider_id: ProviderId::Codex,
                    connection: Connection::BrowserRedirect {
                        authorization_url: "pasted-code".into(),
                    },
                })
                .is_empty()
        );
    }

    #[test]
    fn abandoned_flow_warns_on_stale_failures() {
        let mut model = connecting(ProviderId::Codex);
        let _ = model.update(Msg::DialogClosed);

        let cmds = model.update(Msg::InitFinished(ProviderId::Codex, Err(error("boom"))));
        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));

        let cmds = model.update(Msg::FinalizeFinished(ProviderId::Codex, Err(error("boom"))));
        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));
    }

    #[test]
    fn abandoned_flow_success_still_refreshes_the_page() {
        let mut model = connecting(ProviderId::Codex);
        let _ = model.update(Msg::DialogClosed);

        let cmds = model.update(Msg::FinalizeFinished(ProviderId::Codex, Ok(())));
        assert!(matches!(cmds.as_slice(), [Command::FetchConnectors]));
    }

    #[test]
    fn results_from_a_previous_flow_cannot_touch_the_next_session() {
        // Codex flow abandoned mid-finalize, Anthropic flow opened right after.
        let mut model = connecting(ProviderId::Codex);
        let _ = model.update(Msg::DialogClosed);
        let cmds = model.update(Msg::ConnectClicked(ProviderId::Anthropic));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowConnectionDialog(ProviderId::Anthropic),
                Command::InitConnection(ProviderId::Anthropic)
            ]
        ));

        // Codex's late results must not repaint Anthropic's dialog.
        assert!(
            model
                .update(Msg::InitFinished(ProviderId::Codex, Ok(device_code())))
                .is_empty()
        );
        let cmds = model.update(Msg::FinalizeFinished(ProviderId::Codex, Ok(())));
        assert!(matches!(cmds.as_slice(), [Command::FetchConnectors]));
        assert_eq!(model.connecting, Some(ProviderId::Anthropic));
    }
}
