use std::{collections::HashMap, io, time::Duration};

use log::{debug, info};
use oauth2::TokenResponse;
use rmcp::transport::{AuthError, StoredCredentials, auth::OAuthState};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::utils::unix_now;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

const SUCCESS_PAGE: &str = r#"<!doctype html><html><head><title>Scry - Authorization Successful</title><meta charset="utf-8"></head><body style="font-family:-apple-system,BlinkMacSystemFont,sans-serif;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0;background:#111827;color:#f9fafb;"><div style="text-align:center;padding:2rem;"><h1 style="margin-bottom:0.75rem;">Authorization Successful</h1><p style="color:#d1d5db;">You can close this window and return to Scry.</p></div><script>setTimeout(()=>window.close(),2000)</script></body></html>"#;

const FAILURE_PAGE: &str = r#"<!doctype html><html><head><title>Scry - Authorization Failed</title><meta charset="utf-8"></head><body style="font-family:-apple-system,BlinkMacSystemFont,sans-serif;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0;background:#111827;color:#f9fafb;"><div style="text-align:center;padding:2rem;"><h1 style="margin-bottom:0.75rem;">Authorization Failed</h1><p style="color:#d1d5db;">You can close this window and return to Scry for details.</p></div></body></html>"#;

pub(crate) async fn init_oauth_connection(url: &str) -> Result<OAuthCallbackState> {
    let mut oauth_state = OAuthState::new(url, None).await?;

    // Bind before building the authorization URL so the redirect target is
    // already listening once the URL can be opened; port 0 makes the OS pick
    // a free port atomically with the bind.
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(OAuthError::Bind)?;
    let port = listener.local_addr().map_err(OAuthError::Bind)?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    // empty scopes: rmcp selects them from the 401 header or server metadata
    oauth_state
        .start_authorization(&[], &redirect_uri, Some("Scry"))
        .await?;

    info!("Starting OAuth authentication: {url}; callback server listening on port {port}");

    let auth_url = oauth_state.get_authorization_url().await?;

    Ok(OAuthCallbackState {
        state: oauth_state,
        auth_url,
        listener,
    })
}

pub(crate) async fn finalize_oauth_connection(
    mut state: OAuthCallbackState,
) -> Result<StoredCredentials> {
    let (code, csrf) = timeout(CALLBACK_TIMEOUT, wait_for_callback(&state.listener))
        .await
        .map_err(|_| OAuthError::Timeout)??;

    state.state.handle_callback(&code, &csrf).await?;

    // Credentials for the caller to persist; mirror what rmcp itself stores:
    // scopes actually granted and the receipt time its refresh math relies on
    let (client_id, token_response) = state.state.get_credentials().await?;
    let granted_scopes = token_response
        .as_ref()
        .and_then(|token| token.scopes())
        .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();

    info!("finish OAuth authentication.");

    Ok(StoredCredentials::new(
        client_id,
        token_response,
        granted_scopes,
        Some(unix_now()),
    ))
}

/// Accept connections until the authorization redirect arrives; browsers also
/// open speculative connections that never carry the callback.
async fn wait_for_callback(listener: &TcpListener) -> Result<(String, String)> {
    loop {
        let (mut stream, _addr) = listener.accept().await?;

        let mut buf = vec![0u8; 4096];
        // bound the read so an idle speculative connection can't stall the loop
        let n = match timeout(READ_TIMEOUT, stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                debug!("oauth callback: ignoring unreadable connection: {e}");
                continue;
            },
            Err(_) => {
                debug!("oauth callback: ignoring idle connection");
                continue;
            },
        };
        let request = String::from_utf8_lossy(&buf[..n]);
        let first_line = request.lines().next().unwrap_or("");
        let path = first_line.split_whitespace().nth(1).unwrap_or("/");
        let query = path.split_once('?').map(|(_, query)| query).unwrap_or("");

        let mut params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();

        if let Some(code) = params.remove("code") {
            let Some(state) = params.remove("state") else {
                respond(&mut stream, "400 Bad Request", FAILURE_PAGE).await;
                return Err(OAuthError::MissingParam("state"));
            };

            respond(&mut stream, "200 OK", SUCCESS_PAGE).await;
            return Ok((code, state));
        }

        if params.contains_key("error") {
            let error = params.remove("error").unwrap_or_else(|| "unknown".into());
            let description = params
                .remove("error_description")
                .unwrap_or_else(|| "no description".into());
            respond(&mut stream, "400 Bad Request", FAILURE_PAGE).await;
            return Err(OAuthError::Authorization { error, description });
        }

        // not the callback: a speculative connection, favicon, or stray probe
        debug!("oauth callback: ignoring request for {path}");
    }
}

async fn respond(stream: &mut TcpStream, status: &str, page: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        page.len(),
        page
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

pub struct OAuthCallbackState {
    state: OAuthState,
    auth_url: String,
    listener: TcpListener,
}

impl OAuthCallbackState {
    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("failed to start the OAuth callback server: {0}")]
    Bind(io::Error),

    #[error("OAuth callback timed out after {}s", CALLBACK_TIMEOUT.as_secs())]
    Timeout,

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("authorization failed: {error} - {description}")]
    Authorization { error: String, description: String },

    #[error("missing {0} parameter in the OAuth callback")]
    MissingParam(&'static str),

    #[error("MCP authorization failed: {0}")]
    Auth(#[from] AuthError),
}

type Result<T> = std::result::Result<T, OAuthError>;
