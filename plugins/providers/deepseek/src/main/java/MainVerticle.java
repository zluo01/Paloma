import com.github.zluo01.paloma.proto.v1.ProviderError;
import com.github.zluo01.paloma.proto.v1.RequestEvent;
import com.github.zluo01.paloma.proto.v1.ResponseEvent;
import constants.Constants;
import enums.MessageEvent;
import io.vertx.core.DeploymentOptions;
import io.vertx.core.Future;
import io.vertx.core.ThreadingModel;
import io.vertx.core.VerticleBase;
import io.vertx.core.Vertx;
import io.vertx.core.VertxOptions;
import io.vertx.core.eventbus.EventBus;
import io.vertx.core.http.HttpClient;
import io.vertx.core.http.HttpClientOptions;
import io.vertx.core.http.HttpVersion;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.logging.Level;
import java.util.logging.Logger;

public final class MainVerticle extends VerticleBase {
  private static final Logger LOGGER = Logger.getLogger(MainVerticle.class.getSimpleName());

  private EventBus eventBus;
  private DeepSeekClient deepSeekClient;

  @Override
  public Future<?> start() {
    final HttpClient httpClient =
        vertx.createHttpClient(
            new HttpClientOptions()
                .setDefaultHost(Constants.API_HOST)
                .setDefaultPort(443)
                .setSsl(true)
                .setUseAlpn(true)
                .setProtocolVersion(HttpVersion.HTTP_2)
                .setConnectTimeout(30_000)
                .setReadIdleTimeout(900)
                .setKeepAliveTimeout(90)
                .setHttp2KeepAliveTimeout(90));
    eventBus = vertx.eventBus();
    deepSeekClient = new DeepSeekClient(vertx, eventBus, httpClient);

    eventBus.registerDefaultCodec(RequestEvent.class, new EventCodec<>(RequestEvent.class));
    eventBus.registerDefaultCodec(ResponseEvent.class, new EventCodec<>(ResponseEvent.class));

    eventBus
        .<RequestEvent>consumer(MessageEvent.REQUEST_EVENT.name())
        .handler(
            event -> {
              final long eventId = event.body().getEventId();
              final var backendId =
                  event.body().hasBackendId() ? event.body().getBackendId() : null;
              try {
                switch (event.body().getPayloadCase()) {
                  case HANDSHAKE_REQUEST -> deepSeekClient.handshakeRequest(eventId);
                  case INITIALIZE_BACKENDS_REQUEST ->
                      deepSeekClient.initBackendsRequest(
                          eventId, event.body().getInitializeBackendsRequest());
                  case INIT_BACKEND_REQUEST ->
                      deepSeekClient.initBackendRequest(
                          eventId, event.body().getInitBackendRequest());
                  case REMOVE_BACKEND_REQUEST -> deepSeekClient.removeBackend(eventId, backendId);
                  case INIT_CONNECTION_REQUEST -> deepSeekClient.initConnection(eventId, backendId);
                  case FINALIZE_CONNECTION_REQUEST ->
                      deepSeekClient.finalizeConnection(
                          eventId, backendId, event.body().getFinalizeConnectionRequest());
                  case CANCEL_CONNECTION_REQUEST -> deepSeekClient.cancelConnection(eventId);
                  case CHAT_REQUEST ->
                      deepSeekClient.chat(eventId, backendId, event.body().getChatRequest());
                  case CANCEL_CHAT_REQUEST ->
                      deepSeekClient.cancelChat(eventId, event.body().getCancelChatRequest());
                  case LIST_MODELS_REQUEST -> deepSeekClient.listModel(eventId);
                  case HEALTH_STATUS_REQUEST -> deepSeekClient.healthStatus(eventId);
                  case BACKEND_INIT_ERROR_REQUEST -> deepSeekClient.backendInitError(eventId);
                  case BACKEND_HEALTH_STATUS_REQUEST -> deepSeekClient.backendHealthStatus(eventId);
                  default -> {
                    LOGGER.severe(
                        "request "
                            + eventId
                            + " has no payload: host bug or newer protocol version");
                    replyError(eventId, "unsupported or missing request payload");
                  }
                }
              } catch (Exception e) {
                LOGGER.log(Level.SEVERE, "handler failed for request " + eventId, e);
                final var msg = e.getMessage();
                replyError(eventId, msg == null || msg.isBlank() ? "internal plugin error" : msg);
              }
            });

    return vertx
        .deployVerticle(
            new MessageWriterVerticle(),
            new DeploymentOptions()
                .setThreadingModel(ThreadingModel.WORKER)
                .setWorkerPoolName("MessageWriter")
                .setWorkerPoolSize(1))
        .onSuccess(_ -> startStdinReader());
  }

  private void replyError(final long eventId, final String message) {
    eventBus.send(
        MessageEvent.RESPONSE_EVENT.name(),
        ResponseEvent.newBuilder()
            .setEventId(eventId)
            .setProviderError(ProviderError.newBuilder().setError(message))
            .build());
  }

  private void startStdinReader() {
    Thread.ofPlatform()
        .name("stdin-reader")
        .daemon()
        .start(
            () -> {
              try (InputStream in = System.in) {
                RequestEvent event;
                while ((event = RequestEvent.parseDelimitedFrom(in)) != null) {
                  final var e = event;
                  context.runOnContext(
                      _ ->
                          eventBus.send(
                              MessageEvent.REQUEST_EVENT.name(), e)); // hop to the event loop
                }
              } catch (IOException ex) {
                LOGGER.log(Level.SEVERE, "stdin stream failed", ex);
              }
              // EOF: host is gone; drain dispatched requests and queued writes, then exit
              context.runOnContext(
                  _ ->
                      eventBus
                          .request(MessageEvent.FLUSH.name(), null)
                          .onComplete(_ -> vertx.close()));
            });
  }

  /** Worker Verticle only for flushing Protobuf response back in synchronize order. */
  private static class MessageWriterVerticle extends VerticleBase {
    private final OutputStream stdout = new FileOutputStream(FileDescriptor.out);

    @Override
    public Future<?> start() {
      vertx
          .eventBus()
          .<ResponseEvent>consumer(
              MessageEvent.RESPONSE_EVENT.name(),
              event -> {
                try {
                  event.body().writeDelimitedTo(stdout);
                  stdout.flush();
                } catch (IOException e) {
                  LOGGER.log(Level.SEVERE, "stdout write failed; shutting down", e);
                  // vertx.close() deadlocks when called from this worker context
                  Thread.ofPlatform().daemon().start(vertx::close);
                }
              });
      vertx.eventBus().consumer(MessageEvent.FLUSH.name(), message -> message.reply(null));
      return Future.succeededFuture();
    }
  }

  static void main() {
    final Vertx vertx = Vertx.vertx(new VertxOptions().setEventLoopPoolSize(2));
    vertx.exceptionHandler(throwable -> LOGGER.log(Level.SEVERE, "Unhandled exception", throwable));
    vertx
        .deployVerticle(new MainVerticle())
        .onFailure(
            error -> {
              LOGGER.log(Level.SEVERE, "Fail to start.", error);
              vertx.close();
            });
  }
}
