use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
};
use tonic::transport::server::Connected;

pub struct PipeConnection(NamedPipeServer);

impl Connected for PipeConnection {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl AsyncRead for PipeConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for PipeConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// Accepted connections on `address`; a fresh pipe instance replaces each
/// accepted one so a client can always connect. Fails if the pipe already
/// exists, which doubles as the single-instance lock.
pub fn incoming(
    address: &str,
) -> io::Result<impl Stream<Item = io::Result<PipeConnection>> + use<>> {
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .create(address)?;
    let address = address.to_owned();
    Ok(futures::stream::try_unfold(
        (first, address),
        |(server, address)| async move {
            server.connect().await?;
            let next = ServerOptions::new().create(&address)?;
            Ok(Some((PipeConnection(server), (next, address))))
        },
    ))
}

#[cfg(test)]
mod tests {
    use hyper_util::rt::TokioIo;
    use paloma_binding_protocol::v1::{self as pb, binding_client::BindingClient};
    use paloma_core::AppContext;
    use tokio::net::windows::named_pipe::ClientOptions;
    use tonic::transport::Endpoint;
    use uuid::Uuid;

    use crate::service::BindingService;

    #[tokio::test(flavor = "multi_thread")]
    async fn health_and_connectors_over_the_pipe() {
        let data_dir = std::env::temp_dir().join(format!("paloma-test-{}", Uuid::now_v7()));
        let app = AppContext::build(data_dir.clone())
            .await
            .expect("build core");

        let pipe = format!(r"\\.\pipe\paloma-test-{}", Uuid::now_v7());
        let incoming = super::incoming(&pipe).expect("create pipe");
        let server = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(pb::binding_server::BindingServer::new(BindingService::new(
                    app,
                )))
                .serve_with_incoming(incoming),
        );

        let channel = Endpoint::try_from("http://localhost")
            .expect("endpoint")
            .connect_with_connector(tower::service_fn(move |_| {
                let pipe = pipe.clone();
                async move { ClientOptions::new().open(&pipe).map(TokioIo::new) }
            }))
            .await
            .expect("connect over pipe");
        let mut client = BindingClient::new(channel);

        client
            .health(pb::HealthRequest {})
            .await
            .expect("health rpc");

        let connectors = client
            .available_connectors(pb::AvailableConnectorsRequest {})
            .await
            .expect("available_connectors rpc")
            .into_inner()
            .connectors;
        assert!(
            !connectors.is_empty(),
            "built-in providers should register connectors"
        );

        server.abort();
        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
