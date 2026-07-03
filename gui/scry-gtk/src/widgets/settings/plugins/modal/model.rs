use std::collections::{HashMap, HashSet};

use gtk4::glib;
use scry_core::{Plugin, PluginArgs, Transport};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Kind {
    #[default]
    Local,
    Remote,
}

#[derive(Default)]
pub(super) struct Model {
    pub name: String,
    pub kind: Kind,
    pub command: String,
    pub args: String,
    pub url: String,
    pub requires_auth: bool,
    pub timeout: u32,
    pub env: String,
    /// Preserved from the edited plugin; the form doesn't expose it.
    pub disabled: bool,
    /// Names the user may not choose, so duplicates are rejected.
    pub taken: HashSet<String>,
    /// Whether validation results may show yet (false while mid-typing).
    pub settled: bool,
    /// A submission is in flight.
    pub submitting: bool,
    /// flag for edit not create new tool.
    pub editing: bool,
}

impl Model {
    pub(super) fn from_plugin(taken: HashSet<String>, initial: Plugin) -> Self {
        let mut form = Self {
            name: initial.name,
            disabled: initial.disabled,
            timeout: initial.timeout,
            taken,
            // Pre-filled values are valid input, so show their verdict at once.
            settled: true,
            editing: true,
            ..Self::default()
        };

        if !initial.env.is_empty() {
            form.env = serde_json::to_string(&initial.env).unwrap_or_default();
        }
        match initial.args {
            PluginArgs::Local { command, args } => {
                form.kind = Kind::Local;
                form.command = command;
                if !args.is_empty() {
                    form.args = serde_json::to_string(&args).unwrap_or_default();
                }
            },
            PluginArgs::Remote { url, requires_auth } => {
                form.kind = Kind::Remote;
                form.url = url;
                form.requires_auth = requires_auth;
            },
        }

        form
    }

    pub(super) fn to_plugin(&self) -> Plugin {
        let (transport, args) = match self.kind {
            Kind::Local => (
                Transport::Local,
                PluginArgs::Local {
                    command: self.command.trim().to_string(),
                    args: parse_args(&self.args).unwrap_or_default(),
                },
            ),
            Kind::Remote => (
                Transport::Http,
                PluginArgs::Remote {
                    url: self.url.trim().to_string(),
                    requires_auth: self.requires_auth,
                },
            ),
        };
        Plugin {
            name: self.name.trim().to_string(),
            transport,
            timeout: self.timeout,
            disabled: self.disabled,
            env: parse_env(&self.env).unwrap_or_default(),
            args,
        }
    }

    pub(super) fn validate(&self) -> Validation {
        let mut v = Validation::default();

        let name = self.name.trim();
        let duplicate = self.taken.contains(name);
        if duplicate {
            v.name = Some("A plugin with this name already exists.");
        }
        let name_ok = !name.is_empty() && !duplicate;

        let source_ok = match self.kind {
            Kind::Local => {
                let args_invalid = parse_args(&self.args).is_err();
                if args_invalid {
                    v.args = Some(r#"Must be a JSON array like ["--flag", "value"]."#);
                }
                !self.command.trim().is_empty() && !args_invalid
            },
            Kind::Remote => {
                let url = self.url.trim();
                let invalid = !url.is_empty() && !is_valid_url(url);
                if invalid {
                    v.url = Some("Must be a valid http(s) URL.");
                }
                !url.is_empty() && !invalid
            },
        };

        let env_invalid = parse_env(&self.env).is_err();
        if env_invalid {
            v.env = Some(r#"Must be a JSON object like {"KEY": "value"}."#);
        }

        v.ok = name_ok && source_ok && !env_invalid;
        v
    }

    /// Plugin dialog workflow:
    ///
    /// - Field edit: `NameChanged` / `CommandChanged` / `ArgsChanged` / `UrlChanged` / `EnvChanged -> RenderForm -> ScheduleValidation`.
    /// - Validation timer: `ValidationDebounceElapsed -> RenderForm`.
    /// - Add or Save button: `SubmitClicked -> PersistPlugin -> PluginSaveFinished`.
    /// - Successful save: `PluginSaveFinished(Ok) -> CloseDialog`.
    /// - Failed save: `PluginSaveFinished(Err) -> ShowErrorBanner -> RenderForm`.
    pub(super) fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::NameChanged(s) => {
                self.name = s;
                edited(self)
            },
            Msg::CommandChanged(s) => {
                self.command = s;
                edited(self)
            },
            Msg::ArgsChanged(s) => {
                self.args = s;
                edited(self)
            },
            Msg::UrlChanged(s) => {
                self.url = s;
                edited(self)
            },
            Msg::EnvChanged(s) => {
                self.env = s;
                edited(self)
            },
            Msg::KindChanged(kind) => {
                self.kind = kind;
                self.settled = true;
                vec![Command::RenderForm]
            },
            Msg::ValidationDebounceElapsed => {
                self.settled = true;
                vec![Command::RenderForm]
            },
            Msg::SubmitClicked {
                timeout,
                requires_auth,
            } => {
                self.timeout = timeout;
                self.requires_auth = requires_auth;
                if self.settled && self.validate().ok {
                    self.submitting = true;
                    vec![
                        Command::PersistPlugin(self.to_plugin()),
                        Command::RenderForm,
                    ]
                } else {
                    vec![]
                }
            },
            Msg::PluginSaveFinished(Ok(())) => vec![Command::CloseDialog],
            Msg::PluginSaveFinished(Err(message)) => {
                self.submitting = false;
                vec![Command::ShowErrorBanner(message), Command::RenderForm]
            },
            Msg::CancelClicked => vec![Command::CloseDialog],
        }
    }
}

pub(super) enum Msg {
    NameChanged(String),
    KindChanged(Kind),
    CommandChanged(String),
    ArgsChanged(String),
    UrlChanged(String),
    EnvChanged(String),
    SubmitClicked { timeout: u32, requires_auth: bool },
    ValidationDebounceElapsed,
    PluginSaveFinished(Result<(), String>),
    CancelClicked,
}

pub(super) enum Command {
    RenderForm,
    ScheduleValidation,
    PersistPlugin(Plugin),
    ShowErrorBanner(String),
    CloseDialog,
}

/// A field changed: hide validation until the user pauses, then revalidate.
fn edited(form: &mut Model) -> Vec<Command> {
    form.settled = false;
    vec![Command::RenderForm, Command::ScheduleValidation]
}

/// Per-field error message (`None` is ok) plus the overall verdict.
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Validation {
    pub name: Option<&'static str>,
    pub args: Option<&'static str>,
    pub url: Option<&'static str>,
    pub env: Option<&'static str>,
    pub ok: bool,
}

/// Parse the optional args field; empty means no args.
fn parse_args(text: &str) -> Result<Vec<String>, serde_json::Error> {
    let text = text.trim();
    if text.is_empty() {
        Ok(Vec::new())
    } else {
        serde_json::from_str(text)
    }
}

/// Parse the optional env field; empty means no env.
fn parse_env(text: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    let text = text.trim();
    if text.is_empty() {
        Ok(HashMap::new())
    } else {
        serde_json::from_str(text)
    }
}

/// True for an absolute http(s) URL with a non-empty host.
fn is_valid_url(text: &str) -> bool {
    match glib::Uri::parse(text, glib::UriFlags::NONE) {
        Ok(uri) => {
            matches!(uri.scheme().as_str(), "http" | "https")
                && uri.host().is_some_and(|host| !host.is_empty())
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_form() -> Model {
        Model {
            name: "fs".into(),
            kind: Kind::Local,
            command: "node".into(),
            args: r#"["server.js"]"#.into(),
            settled: true,
            ..Model::default()
        }
    }

    fn remote_form() -> Model {
        Model {
            name: "api".into(),
            kind: Kind::Remote,
            url: "https://example.com/mcp".into(),
            settled: true,
            ..Model::default()
        }
    }

    #[test]
    fn local_plugin_prefills_edit_form() {
        let env = HashMap::from([("NODE_ENV".into(), "test".into())]);
        let plugin = Plugin {
            name: "fs".into(),
            transport: Transport::Local,
            timeout: 120,
            disabled: true,
            env: env.clone(),
            args: PluginArgs::Local {
                command: "node".into(),
                args: vec!["server.js".into(), "--stdio".into()],
            },
        };

        let form = Model::from_plugin(HashSet::from(["other".into()]), plugin);

        assert!(form.editing);
        assert!(form.settled);
        assert!(form.disabled);
        assert!(form.taken.contains("other"));
        assert!(matches!(form.kind, Kind::Local));
        assert_eq!(form.name, "fs");
        assert_eq!(form.command, "node");
        assert_eq!(form.timeout, 120);
        assert_eq!(
            form.args,
            serde_json::to_string(&vec!["server.js", "--stdio"]).unwrap()
        );
        assert_eq!(form.env, serde_json::to_string(&env).unwrap());
    }

    #[test]
    fn remote_plugin_prefills_edit_form() {
        let plugin = Plugin {
            name: "api".into(),
            transport: Transport::Http,
            timeout: 30,
            disabled: false,
            env: HashMap::new(),
            args: PluginArgs::Remote {
                url: "https://example.com/mcp".into(),
                requires_auth: true,
            },
        };

        let form = Model::from_plugin(HashSet::new(), plugin);

        assert!(form.editing);
        assert!(form.settled);
        assert!(matches!(form.kind, Kind::Remote));
        assert_eq!(form.name, "api");
        assert_eq!(form.url, "https://example.com/mcp");
        assert!(form.requires_auth);
        assert_eq!(form.timeout, 30);
        assert!(form.command.is_empty());
        assert!(form.args.is_empty());
        assert!(form.env.is_empty());
    }

    #[test]
    fn valid_local_form_is_ok() {
        let v = local_form().validate();
        assert!(v.ok);
        assert_eq!(
            v,
            Validation {
                ok: true,
                ..Validation::default()
            }
        );
    }

    #[test]
    fn local_form_builds_local_plugin() {
        let plugin = local_form().to_plugin();
        assert_eq!(plugin.transport, Transport::Local);
        assert!(matches!(
            plugin.args,
            PluginArgs::Local { ref command, ref args }
                if command == "node" && args[..] == ["server.js"]
        ));
    }

    #[test]
    fn valid_remote_form_is_ok() {
        assert!(remote_form().validate().ok);
    }

    #[test]
    fn remote_form_builds_remote_plugin() {
        let mut form = remote_form();
        form.requires_auth = true;
        let plugin = form.to_plugin();
        assert!(matches!(
            plugin.args,
            PluginArgs::Remote { ref url, requires_auth: true }
                if url == "https://example.com/mcp"
        ));
    }

    #[test]
    fn duplicate_name_is_flagged() {
        let mut form = local_form();
        form.taken = HashSet::from(["fs".into()]);
        let v = form.validate();
        assert!(!v.ok);
        assert!(v.name.is_some());
    }

    #[test]
    fn empty_command_is_not_ok() {
        let mut form = local_form();
        form.command = "  ".into();
        assert!(!form.validate().ok);
    }

    #[test]
    fn invalid_args_json_is_flagged() {
        let mut form = local_form();
        form.args = "not json".into();
        let v = form.validate();
        assert!(!v.ok);
        assert!(v.args.is_some());
    }

    #[test]
    fn empty_url_is_not_ok() {
        let mut form = remote_form();
        form.url = String::new();
        assert!(!form.validate().ok);
    }

    #[test]
    fn invalid_url_is_flagged() {
        let mut form = remote_form();
        form.url = "ftp://nope".into();
        let v = form.validate();
        assert!(!v.ok);
        assert!(v.url.is_some());
    }

    #[test]
    fn malformed_url_is_invalid() {
        assert!(!is_valid_url("http://[::1"));
    }

    #[test]
    fn invalid_env_json_is_flagged() {
        let mut form = local_form();
        form.env = "[]".into();
        let v = form.validate();
        assert!(!v.ok);
        assert!(v.env.is_some());
    }

    #[test]
    fn editing_a_field_unsettles_and_schedules_validation() {
        let mut form = local_form();
        let cmds = form.update(Msg::NameChanged("new".into()));
        assert!(!form.settled);
        assert!(matches!(
            cmds.as_slice(),
            [Command::RenderForm, Command::ScheduleValidation]
        ));
    }

    #[test]
    fn submit_when_valid_persists() {
        let mut form = local_form();
        let cmds = form.update(Msg::SubmitClicked {
            timeout: 300,
            requires_auth: false,
        });
        assert!(form.submitting);
        assert!(matches!(
            cmds.as_slice(),
            [Command::PersistPlugin(_), Command::RenderForm]
        ));
    }

    #[test]
    fn submit_while_unsettled_does_nothing() {
        let mut form = local_form();
        form.settled = false;
        let cmds = form.update(Msg::SubmitClicked {
            timeout: 300,
            requires_auth: false,
        });
        assert!(!form.submitting);
        assert!(cmds.is_empty());
    }

    #[test]
    fn failed_submit_shows_banner_and_reenables() {
        let mut form = local_form();
        form.submitting = true;
        let cmds = form.update(Msg::PluginSaveFinished(Err("taken".into())));
        assert!(!form.submitting);
        assert!(matches!(
            cmds.as_slice(),
            [Command::ShowErrorBanner(_), Command::RenderForm]
        ));
    }
}
