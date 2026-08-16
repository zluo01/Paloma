use paloma_core::{
    AppError, ConnectionPayload, Connector, ProviderAuthMethod, ProviderBackendId,
    connection_payload,
};

#[derive(Default)]
pub(super) struct Model {
    pub(super) connectors: Vec<Connector>,
    pub(super) connecting: Option<ProviderBackendId>,
}

pub(super) enum Msg {
    ConnectorsFetched(Result<Vec<Connector>, AppError>),
    DisconnectClicked(ProviderBackendId),
    DisconnectConfirmed(ProviderBackendId),
    DisconnectFinished(ProviderBackendId, Result<(), AppError>),
    // dialog
    ConnectClicked(ProviderBackendId),
    InitFinished(ProviderBackendId, Result<ConnectionPayload, AppError>),
    ConnectionSubmitted {
        provider_auth_method: ProviderAuthMethod,
        provider_backend_id: ProviderBackendId,
        payload: String,
    },
    FinalizeFinished {
        provider_backend_id: ProviderBackendId,
        response: Result<(), AppError>,
    },
    CloseDialogClicked,
    DialogClosed(ProviderBackendId),
}

pub(super) enum Command {
    Render,
    FetchConnectors,
    ShowDisconnectConfirmation(ProviderBackendId),
    DisconnectProvider(ProviderBackendId),
    Warn(String),
    ShowErrorDialog(String),
    // dialog
    InitConnection(ProviderBackendId),
    ShowConnectionDialog(ProviderBackendId),
    ShowLoading,
    ShowChallenge {
        verification_uri: String,
        user_code: String,
    },
    ShowManualInput {
        provider_backend_id: ProviderBackendId,
        instructions_url: Option<String>,
    },
    ShowOauth {
        provider_backend_id: ProviderBackendId,
        authorization_url: String,
    },
    ShowError(String),
    ShowSuccess,
    FinalizeConnection {
        provider_auth_method: ProviderAuthMethod,
        provider_backend_id: ProviderBackendId,
        payload: String,
    },
    CloseConnectionDialog,
    DropConnectionDialog(ProviderBackendId),
}

impl Model {
    /// Services page workflow:
    ///
    /// - Page refresh: `ConnectorsFetched -> Render`.
    /// - Disconnect button: `DisconnectClicked -> ShowDisconnectConfirmation
    ///   -> DisconnectConfirmed -> DisconnectProvider -> DisconnectFinished
    ///   -> FetchConnectors -> ConnectorsFetched -> Render`.
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
            Msg::ConnectClicked(id) => {
                if self.connecting.is_some() {
                    return vec![];
                }
                self.connecting = Some(id.clone());
                vec![
                    Command::ShowConnectionDialog(id.clone()),
                    Command::InitConnection(id),
                ]
            },
            Msg::InitFinished(provider_backend_id, result) => {
                if self.connecting.as_ref() != Some(&provider_backend_id) {
                    return match result {
                        Ok(_) => vec![],
                        Err(e) => vec![Command::Warn(format!(
                            "stale connect failure for {provider_backend_id}: {e}"
                        ))],
                    };
                }
                match result {
                    Ok(ConnectionPayload {
                        payload: Some(connection_payload::Payload::DeviceCode(device_code)),
                    }) => vec![
                        Command::ShowChallenge {
                            verification_uri: device_code.verification_url,
                            user_code: device_code.user_code,
                        },
                        Command::FinalizeConnection {
                            provider_auth_method: ProviderAuthMethod::DeviceCode,
                            provider_backend_id,
                            payload: device_code.transaction_payload,
                        },
                    ],
                    Ok(ConnectionPayload {
                        payload: Some(connection_payload::Payload::ManualInput(manual_input)),
                    }) => vec![Command::ShowManualInput {
                        provider_backend_id,
                        instructions_url: manual_input.instructions_url,
                    }],
                    Ok(ConnectionPayload {
                        payload: Some(connection_payload::Payload::BrowserRedirect(redirect)),
                    }) => {
                        vec![Command::ShowOauth {
                            provider_backend_id,
                            authorization_url: redirect.authorization_url,
                        }]
                    },
                    // should not happen, this indicates a bug.
                    Ok(ConnectionPayload { payload: None }) => vec![Command::ShowError(
                        "Provider returned an empty connection payload.".to_string(),
                    )],
                    Err(e) => vec![Command::ShowError(e.to_string())],
                }
            },
            Msg::ConnectionSubmitted {
                provider_auth_method,
                provider_backend_id,
                payload,
            } => {
                if self.connecting.as_ref() != Some(&provider_backend_id) {
                    return vec![];
                }
                vec![
                    Command::ShowLoading,
                    Command::FinalizeConnection {
                        provider_auth_method,
                        provider_backend_id,
                        payload,
                    },
                ]
            },
            Msg::FinalizeFinished {
                provider_backend_id,
                response,
            } => {
                if self.connecting.as_ref() == Some(&provider_backend_id) {
                    match response {
                        Ok(()) => vec![Command::ShowSuccess, Command::FetchConnectors],
                        Err(e) => vec![Command::ShowError(e.to_string())],
                    }
                } else {
                    match response {
                        // The backend finished after the dialog moved on; the
                        // connection exists either way, so still refresh.
                        Ok(()) => vec![Command::FetchConnectors],
                        Err(e) => vec![Command::Warn(format!(
                            "stale connect failure for {provider_backend_id}: {e}"
                        ))],
                    }
                }
            },
            Msg::CloseDialogClicked => vec![Command::CloseConnectionDialog],
            Msg::DialogClosed(provider_backend_id) => {
                self.connecting = None;
                vec![Command::DropConnectionDialog(provider_backend_id)]
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use paloma_core::{BrowserRedirect, DeviceCode, ManualInput};

    use super::*;

    fn error(message: &str) -> AppError {
        std::io::Error::other(message).into()
    }

    fn codex() -> ProviderBackendId {
        ProviderBackendId {
            provider_id: "OpenAI".into(),
            backend_id: "Codex".into(),
        }
    }

    fn openai() -> ProviderBackendId {
        ProviderBackendId {
            provider_id: "OpenAI".into(),
            backend_id: "OpenAI API".into(),
        }
    }

    fn anthropic() -> ProviderBackendId {
        ProviderBackendId {
            provider_id: "Anthropic".into(),
            backend_id: "Anthropic API".into(),
        }
    }

    fn disconnected(id: ProviderBackendId) -> Connector {
        Connector {
            id,
            description: String::new(),
            icon: None,
            connection: None,
        }
    }

    fn device_code() -> ConnectionPayload {
        ConnectionPayload {
            payload: Some(connection_payload::Payload::DeviceCode(DeviceCode {
                verification_url: "https://example.com/device".into(),
                user_code: "ABCD-1234".into(),
                transaction_payload: "txn-1".into(),
            })),
        }
    }

    fn manual_input(instructions_url: Option<&str>) -> ConnectionPayload {
        ConnectionPayload {
            payload: Some(connection_payload::Payload::ManualInput(ManualInput {
                api_key: String::new(),
                instructions_url: instructions_url.map(str::to_string),
            })),
        }
    }

    fn browser_redirect(authorization_url: &str) -> ConnectionPayload {
        ConnectionPayload {
            payload: Some(connection_payload::Payload::BrowserRedirect(
                BrowserRedirect {
                    authorization_url: authorization_url.into(),
                },
            )),
        }
    }

    /// Walk the model to the state where `id`'s connection dialog is open.
    fn connecting(id: ProviderBackendId) -> Model {
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
        let cmds = model.update(Msg::ConnectorsFetched(Ok(vec![disconnected(codex())])));

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

        let cmds = model.update(Msg::DisconnectClicked(codex()));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowDisconnectConfirmation(id)] if *id == codex()
        ));

        let cmds = model.update(Msg::DisconnectConfirmed(codex()));
        assert!(matches!(
            cmds.as_slice(),
            [Command::DisconnectProvider(id)] if *id == codex()
        ));

        let cmds = model.update(Msg::DisconnectFinished(codex(), Ok(())));
        assert!(matches!(cmds.as_slice(), [Command::FetchConnectors]));

        let cmds = model.update(Msg::ConnectorsFetched(Ok(vec![])));
        assert!(matches!(cmds.as_slice(), [Command::Render]));
    }

    #[test]
    fn disconnect_workflow_failure_shows_an_error_dialog() {
        let mut model = Model::default();
        let _ = model.update(Msg::DisconnectClicked(codex()));
        let _ = model.update(Msg::DisconnectConfirmed(codex()));

        let cmds = model.update(Msg::DisconnectFinished(codex(), Err(error("nope"))));
        assert!(matches!(cmds.as_slice(), [Command::ShowErrorDialog(_)]));
    }

    #[test]
    fn connect_click_opens_the_dialog_and_inits_connection() {
        let mut model = Model::default();
        let cmds = model.update(Msg::ConnectClicked(codex()));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowConnectionDialog(dialog_id),
                Command::InitConnection(init_id)
            ] if *dialog_id == codex() && *init_id == codex()
        ));
        assert_eq!(model.connecting, Some(codex()));
    }

    #[test]
    fn connect_click_is_ignored_while_a_dialog_is_open() {
        let mut model = connecting(codex());
        assert!(model.update(Msg::ConnectClicked(openai())).is_empty());
        assert_eq!(model.connecting, Some(codex()));
    }

    #[test]
    fn device_code_workflow_shows_challenge_finalizes_and_closes() {
        let mut model = connecting(codex());

        // Init returned a device-code challenge: the code is shown and the
        // finalize wait starts immediately.
        let cmds = model.update(Msg::InitFinished(codex(), Ok(device_code())));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowChallenge { user_code, .. },
                Command::FinalizeConnection {
                    provider_auth_method: ProviderAuthMethod::DeviceCode,
                    provider_backend_id,
                    payload,
                }
            ] if user_code == "ABCD-1234"
                && *provider_backend_id == codex()
                && payload == "txn-1"
        ));

        // Browser approval resolves the finalize.
        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: codex(),
            response: Ok(()),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowSuccess, Command::FetchConnectors]
        ));

        // The auto-close reports back and frees the flow.
        let cmds = model.update(Msg::DialogClosed(codex()));
        assert!(matches!(
            cmds.as_slice(),
            [Command::DropConnectionDialog(id)] if *id == codex()
        ));
        assert!(model.connecting.is_none());
    }

    #[test]
    fn manual_key_workflow_submits_and_succeeds() {
        let mut model = connecting(anthropic());

        let cmds = model.update(Msg::InitFinished(
            anthropic(),
            Ok(manual_input(Some("https://example.com/keys"))),
        ));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowManualInput {
                provider_backend_id,
                instructions_url: Some(_),
            }] if *provider_backend_id == anthropic()
        ));

        let cmds = model.update(Msg::ConnectionSubmitted {
            provider_auth_method: ProviderAuthMethod::ApiKey,
            provider_backend_id: anthropic(),
            payload: "key".into(),
        });
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowLoading,
                Command::FinalizeConnection {
                    provider_auth_method: ProviderAuthMethod::ApiKey,
                    provider_backend_id,
                    payload,
                }
            ] if *provider_backend_id == anthropic() && payload == "key"
        ));

        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: anthropic(),
            response: Ok(()),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowSuccess, Command::FetchConnectors]
        ));

        let cmds = model.update(Msg::DialogClosed(anthropic()));
        assert!(matches!(
            cmds.as_slice(),
            [Command::DropConnectionDialog(id)] if *id == anthropic()
        ));
        assert!(model.connecting.is_none());
    }

    #[test]
    fn oauth_workflow_submits_the_pasted_code() {
        let mut model = connecting(anthropic());

        let cmds = model.update(Msg::InitFinished(
            anthropic(),
            Ok(browser_redirect("https://example.com/auth")),
        ));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowOauth {
                provider_backend_id,
                ..
            }] if *provider_backend_id == anthropic()
        ));

        let cmds = model.update(Msg::ConnectionSubmitted {
            provider_auth_method: ProviderAuthMethod::BrowserOauth,
            provider_backend_id: anthropic(),
            payload: "pasted-code".into(),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowLoading, Command::FinalizeConnection { .. }]
        ));

        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: anthropic(),
            response: Ok(()),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowSuccess, Command::FetchConnectors]
        ));
    }

    #[test]
    fn failed_init_shows_the_error_page_until_closed() {
        let mut model = connecting(codex());

        let cmds = model.update(Msg::InitFinished(codex(), Err(error("offline"))));
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowError(message)] if message == "offline"
        ));

        // Close on the error page requests the close; the closed dialog then
        // reports back and frees the flow.
        let cmds = model.update(Msg::CloseDialogClicked);
        assert!(matches!(cmds.as_slice(), [Command::CloseConnectionDialog]));

        let cmds = model.update(Msg::DialogClosed(codex()));
        assert!(matches!(
            cmds.as_slice(),
            [Command::DropConnectionDialog(id)] if *id == codex()
        ));
        assert!(model.connecting.is_none());
    }

    #[test]
    fn empty_init_payload_shows_the_error_page() {
        let mut model = connecting(codex());

        let cmds = model.update(Msg::InitFinished(
            codex(),
            Ok(ConnectionPayload { payload: None }),
        ));
        assert!(matches!(cmds.as_slice(), [Command::ShowError(_)]));
    }

    #[test]
    fn failed_finalize_shows_the_error_page() {
        let mut model = connecting(anthropic());
        let _ = model.update(Msg::InitFinished(anthropic(), Ok(manual_input(None))));
        let _ = model.update(Msg::ConnectionSubmitted {
            provider_auth_method: ProviderAuthMethod::ApiKey,
            provider_backend_id: anthropic(),
            payload: "key".into(),
        });

        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: anthropic(),
            response: Err(error("bad key")),
        });
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowError(message)] if message == "bad key"
        ));
    }

    #[test]
    fn abandoned_flow_drops_stale_init_results_and_submits() {
        let mut model = connecting(codex());
        // Esc while still loading.
        let _ = model.update(Msg::DialogClosed(codex()));

        assert!(
            model
                .update(Msg::InitFinished(codex(), Ok(device_code())))
                .is_empty()
        );
        assert!(
            model
                .update(Msg::ConnectionSubmitted {
                    provider_auth_method: ProviderAuthMethod::BrowserOauth,
                    provider_backend_id: codex(),
                    payload: "pasted-code".into(),
                })
                .is_empty()
        );
    }

    #[test]
    fn abandoned_flow_warns_on_stale_failures() {
        let mut model = connecting(codex());
        let _ = model.update(Msg::DialogClosed(codex()));

        let cmds = model.update(Msg::InitFinished(codex(), Err(error("boom"))));
        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));

        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: codex(),
            response: Err(error("boom")),
        });
        assert!(matches!(cmds.as_slice(), [Command::Warn(_)]));
    }

    #[test]
    fn abandoned_flow_success_still_refreshes_the_page() {
        let mut model = connecting(codex());
        let _ = model.update(Msg::DialogClosed(codex()));

        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: codex(),
            response: Ok(()),
        });
        assert!(matches!(cmds.as_slice(), [Command::FetchConnectors]));
    }

    #[test]
    fn results_from_a_previous_flow_cannot_touch_the_next_session() {
        // Codex flow abandoned mid-finalize, Anthropic flow opened right after.
        let mut model = connecting(codex());
        let _ = model.update(Msg::DialogClosed(codex()));
        let cmds = model.update(Msg::ConnectClicked(anthropic()));
        assert!(matches!(
            cmds.as_slice(),
            [
                Command::ShowConnectionDialog(dialog_id),
                Command::InitConnection(init_id)
            ] if *dialog_id == anthropic() && *init_id == anthropic()
        ));

        // Codex's late results must not repaint Anthropic's dialog.
        assert!(
            model
                .update(Msg::InitFinished(codex(), Ok(device_code())))
                .is_empty()
        );
        let cmds = model.update(Msg::FinalizeFinished {
            provider_backend_id: codex(),
            response: Ok(()),
        });
        assert!(matches!(cmds.as_slice(), [Command::FetchConnectors]));
        assert_eq!(model.connecting, Some(anthropic()));
    }
}
