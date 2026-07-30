import com.github.zluo01.paloma.proto.v1.Backend;
import com.github.zluo01.paloma.proto.v1.BackendAuth;
import com.github.zluo01.paloma.proto.v1.BackendHealthStatusResponse;
import com.github.zluo01.paloma.proto.v1.BackendInitErrorResponse;
import com.github.zluo01.paloma.proto.v1.CancelChatRequest;
import com.github.zluo01.paloma.proto.v1.CancelChatResponse;
import com.github.zluo01.paloma.proto.v1.CancelConnectionResponse;
import com.github.zluo01.paloma.proto.v1.ChatRequest;
import com.github.zluo01.paloma.proto.v1.ChatResponse;
import com.github.zluo01.paloma.proto.v1.ConnectionPayload;
import com.github.zluo01.paloma.proto.v1.FinalizeConnectionRequest;
import com.github.zluo01.paloma.proto.v1.FinalizeConnectionResponse;
import com.github.zluo01.paloma.proto.v1.HandshakeResponse;
import com.github.zluo01.paloma.proto.v1.HealthStatusResponse;
import com.github.zluo01.paloma.proto.v1.InitBackendRequest;
import com.github.zluo01.paloma.proto.v1.InitBackendResponse;
import com.github.zluo01.paloma.proto.v1.InitConnectionResponse;
import com.github.zluo01.paloma.proto.v1.InitializeBackendsRequest;
import com.github.zluo01.paloma.proto.v1.InitializeBackendsResponse;
import com.github.zluo01.paloma.proto.v1.ListModelsResponse;
import com.github.zluo01.paloma.proto.v1.ManualInput;
import com.github.zluo01.paloma.proto.v1.Model;
import com.github.zluo01.paloma.proto.v1.ProviderAuth;
import com.github.zluo01.paloma.proto.v1.ProviderAuthMethod;
import com.github.zluo01.paloma.proto.v1.ProviderError;
import com.github.zluo01.paloma.proto.v1.ProviderHealthStatus;
import com.github.zluo01.paloma.proto.v1.RemoveBackendResponse;
import com.github.zluo01.paloma.proto.v1.ResponseEvent;
import com.google.protobuf.ByteString;
import constants.Constants;
import enums.MessageEvent;
import io.netty.handler.codec.http.HttpHeaderValues;
import io.vertx.core.Future;
import io.vertx.core.Vertx;
import io.vertx.core.buffer.Buffer;
import io.vertx.core.eventbus.EventBus;
import io.vertx.core.http.HttpClient;
import io.vertx.core.http.HttpHeaders;
import io.vertx.core.http.HttpMethod;
import io.vertx.core.http.HttpResponseExpectation;
import io.vertx.core.json.JsonArray;
import io.vertx.core.json.JsonObject;
import java.io.IOException;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Consumer;
import java.util.logging.Level;
import java.util.logging.Logger;
import stream.ChatStream;

public final class DeepSeekClient {
  private static final Logger LOGGER = Logger.getLogger(DeepSeekClient.class.getSimpleName());

  private static final ByteString ICON;
  private static final List<Model> MODELS;

  static {
    try (InputStream icon = DeepSeekClient.class.getResourceAsStream("/deepseek.svg");
        InputStream models = DeepSeekClient.class.getResourceAsStream("/models.json")) {
      ICON = ByteString.readFrom(icon);

      final List<Model> parsed = new ArrayList<>();
      for (final Object entry : new JsonArray(Buffer.buffer(models.readAllBytes()))) {
        final JsonObject node = (JsonObject) entry;
        final JsonObject reasoning = node.getJsonObject("reasoning");
        final Model.Builder builder =
            Model.newBuilder()
                .setId(node.getString("id"))
                .setName(node.getString("name"))
                .setDefaultReasoningEffort(reasoning.getString("default_effort"));
        reasoning
            .getJsonArray("supported_efforts")
            .forEach(effort -> builder.addSupportedReasoningEfforts((String) effort));
        parsed.add(builder.build());
      }
      MODELS = List.copyOf(parsed);
    } catch (IOException e) {
      throw new ExceptionInInitializerError(e);
    }
  }

  private final Vertx vertx;
  private final EventBus eventBus;
  private final HttpClient httpClient;
  private final ConcurrentHashMap<String, ChatStream> sessions = new ConcurrentHashMap<>();
  private Optional<String> apiKey;
  private BackendHealth health;

  public DeepSeekClient(final Vertx vertx, final EventBus eventBus, final HttpClient httpClient) {
    this.vertx = vertx;
    this.eventBus = eventBus;
    this.httpClient = httpClient;
    this.apiKey = Optional.empty();
    this.health = BackendHealth.starting();
  }

  void handshakeRequest(final long eventId) {
    reply(
        eventId,
        b ->
            b.setHandshakeResponse(
                HandshakeResponse.newBuilder()
                    .setVersion(Constants.PROTOCOL_VERSION)
                    .setProviderId(Constants.PROVIDER_ID)
                    .setDescription(Constants.DESCRIPTION)
                    .addBackends(
                        Backend.newBuilder()
                            .setAuthKind(ProviderAuthMethod.PROVIDER_AUTH_METHOD_API_KEY)
                            .setBackendId(Constants.BACKEND_ID)
                            .setDescription(Constants.DESCRIPTION)
                            .setIcon(ICON)
                            .build())
                    .build()));
  }

  void initBackendsRequest(final long eventId, final InitializeBackendsRequest request) {
    final List<Future<Void>> inits = new ArrayList<>();
    for (final BackendAuth auth : request.getAuthsList()) {
      if (!Constants.BACKEND_ID.equals(auth.getBackendId())) {
        LOGGER.warning("Skipping auth for unknown backend: " + auth.getBackendId());
        continue;
      }
      inits.add(
          initBackend(auth)
              .onFailure(
                  e -> {
                    LOGGER.log(
                        Level.WARNING,
                        "Failed to initialize " + Constants.BACKEND_ID + " backend; skipping.",
                        e);
                    final var msg = e.getMessage();
                    health =
                        BackendHealth.unhealthy(
                            msg == null || msg.isBlank()
                                ? "Failed to initialize " + Constants.BACKEND_ID + " backend."
                                : msg);
                  }));
    }
    Future.join(inits)
        .onComplete(
            _ ->
                reply(
                    eventId,
                    b ->
                        b.setInitializeBackendsResponse(
                            InitializeBackendsResponse.getDefaultInstance())));
  }

  void initBackendRequest(final long eventId, final InitBackendRequest request) {
    final var backendId = request.getAuth().getBackendId();
    if (!Constants.BACKEND_ID.equals(backendId)) {
      replyError(eventId, "unknown backend: " + (backendId.isEmpty() ? "<missing>" : backendId));
      return;
    }
    initBackend(request.getAuth())
        .onSuccess(
            _ -> {
              final var deferred = health.error();
              if (deferred.isPresent()) {
                replyError(eventId, deferred.get());
              } else {
                reply(
                    eventId,
                    b -> b.setInitBackendResponse(InitBackendResponse.getDefaultInstance()));
              }
            })
        .onFailure(
            e -> {
              final var errorMsg = e.getMessage();
              replyError(
                  eventId,
                  errorMsg == null || errorMsg.isBlank()
                      ? "Failed to initialize " + Constants.BACKEND_ID + " backend."
                      : errorMsg);
            });
  }

  void removeBackend(final long eventId, final String backendId) {
    if (backendId == null) {
      replyError(eventId, "missing required field: backend_id");
      return;
    }
    if (Constants.BACKEND_ID.equals(backendId)
        && health.status() != ProviderHealthStatus.PROVIDER_HEALTH_STATUS_STARTING) {
      this.apiKey = Optional.empty();
      this.health = BackendHealth.starting();
      // aborted streams remove themselves from the map through onClosed
      sessions.values().forEach(stream -> stream.abort("backend removed"));
    } else {
      LOGGER.warning("Backend " + backendId + " was not connected; nothing to remove.");
    }
    reply(eventId, b -> b.setRemoveBackendResponse(RemoveBackendResponse.getDefaultInstance()));
  }

  void initConnection(final long eventId, final String backendId) {
    if (!Constants.BACKEND_ID.equals(backendId)) {
      replyError(eventId, "unknown backend: " + (backendId == null ? "<missing>" : backendId));
      return;
    }
    reply(
        eventId,
        b ->
            b.setInitConnectionResponse(
                InitConnectionResponse.newBuilder()
                    .setConnection(
                        ConnectionPayload.newBuilder()
                            .setManualInput(
                                ManualInput.newBuilder()
                                    .setApiKey("")
                                    .setInstructionsUrl(Constants.INSTRUCTION_URL)
                                    .build())
                            .build())
                    .build()));
  }

  void finalizeConnection(
      final long eventId, final String backendId, final FinalizeConnectionRequest request) {
    if (backendId == null) {
      replyError(eventId, "missing required field: backend_id");
      return;
    }
    if (!Constants.BACKEND_ID.equals(backendId)) {
      replyError(eventId, "unknown backend: " + backendId);
      return;
    }
    if (health.status() != ProviderHealthStatus.PROVIDER_HEALTH_STATUS_STARTING) {
      replyError(eventId, "Backend " + Constants.BACKEND_ID + " is already connected.");
      return;
    }

    if (!request.hasApiKey()) {
      replyError(
          eventId,
          "Unexpected connection input for " + Constants.BACKEND_ID + ": expected an API key.");
      return;
    }

    final var key = request.getApiKey().strip();
    if (key.isBlank()) {
      replyError(eventId, Constants.BACKEND_ID + " API key is required.");
      return;
    }

    reply(
        eventId,
        b ->
            b.setFinalizeConnectionResponse(
                FinalizeConnectionResponse.newBuilder()
                    .setAuth(ProviderAuth.newBuilder().setApiKey(key).build())
                    .build()));
  }

  void cancelConnection(final long eventId) {
    reply(
        eventId, b -> b.setCancelConnectionResponse(CancelConnectionResponse.getDefaultInstance()));
  }

  void listModel(final long eventId) {
    reply(
        eventId,
        b -> b.setListModelsResponse(ListModelsResponse.newBuilder().addAllModels(MODELS).build()));
  }

  void healthStatus(final long eventId) {
    reply(
        eventId,
        b ->
            b.setHealthStatusResponse(
                HealthStatusResponse.newBuilder().setHealthStatus(health.status()).build()));
  }

  void backendHealthStatus(final long eventId) {
    final var builder = BackendHealthStatusResponse.newBuilder();
    if (health.status() != ProviderHealthStatus.PROVIDER_HEALTH_STATUS_STARTING) {
      builder.addStatus(health.status());
    }
    reply(eventId, b -> b.setBackendHealthStatusResponse(builder.build()));
  }

  void backendInitError(final long eventId) {
    final var builder = BackendInitErrorResponse.newBuilder();
    health.error().ifPresent(builder::setError);
    reply(eventId, b -> b.setBackendInitErrorResponse(builder.build()));
  }

  private Future<Void> initBackend(final BackendAuth auth) {
    if (!auth.hasAuth() || !auth.getAuth().hasApiKey()) {
      return Future.failedFuture(new IllegalStateException("Expected an API key auth."));
    }

    if (apiKey.isPresent()) {
      LOGGER.warning(
          "Backend " + Constants.BACKEND_ID + " re-initialized, replacing the existing key.");
    }

    final var key = auth.getAuth().getApiKey().strip();
    if (key.isBlank()) {
      apiKey = Optional.empty();
      health = BackendHealth.unhealthy("Invalid api key.");
      return Future.succeededFuture();
    }
    return healthCheck(key)
        .onSuccess(
            _ -> {
              apiKey = Optional.of(key);
              health = BackendHealth.running();
            })
        .otherwise(
            e -> {
              apiKey = Optional.empty();
              final var msg = e.getMessage();
              health =
                  BackendHealth.unhealthy(
                      msg == null || msg.isBlank()
                          ? "Fail to connect to " + Constants.BACKEND_ID + "."
                          : msg);
              return null;
            });
  }

  void chat(final long eventId, final String backendId, final ChatRequest request) {
    if (!Constants.BACKEND_ID.equals(backendId) || apiKey.isEmpty()) {
      chatError(
          eventId,
          "backend " + (backendId == null ? "<UNKNOWN>" : backendId) + " is not initialized");
      return;
    }
    final var sessionId = request.getSessionId();
    final var stream =
        new ChatStream(
            vertx,
            eventBus,
            httpClient,
            eventId,
            apiKey.get(),
            closed -> sessions.remove(sessionId, closed));
    final var previous = sessions.put(sessionId, stream);
    if (previous != null) {
      LOGGER.severe(
          "session "
              + sessionId
              + " already had an in-flight chat; cancelling the previous task. This indicates a bug.");
      previous.abort("chat cancelled");
    }
    try {
      stream.start(DeepSeekCodec.buildRequestBody(request));
    } catch (Exception e) {
      LOGGER.log(Level.SEVERE, "chat start failed for session " + sessionId, e);
      final var msg = e.getMessage();
      stream.abort(msg == null || msg.isBlank() ? "internal plugin error" : msg);
    }
  }

  void cancelChat(final long eventId, final CancelChatRequest request) {
    final var stream = sessions.remove(request.getSessionId());
    if (stream != null) {
      stream.abort("chat cancelled");
    } else {
      LOGGER.warning("no in-flight chat for session " + request.getSessionId());
    }
    reply(eventId, b -> b.setCancelChatResponse(CancelChatResponse.getDefaultInstance()));
  }

  private void chatError(final long eventId, final String msg) {
    reply(eventId, b -> b.setChatResponse(ChatResponse.newBuilder().setError(msg)));
  }

  private void reply(final long eventId, final Consumer<ResponseEvent.Builder> payload) {
    final var builder = ResponseEvent.newBuilder().setEventId(eventId);
    payload.accept(builder);
    eventBus.send(MessageEvent.RESPONSE_EVENT.name(), builder.build());
  }

  private void replyError(final long eventId, final String msg) {
    reply(eventId, b -> b.setProviderError(ProviderError.newBuilder().setError(msg)));
  }

  private Future<Void> healthCheck(final String apiKey) {
    return httpClient
        .request(HttpMethod.GET, "/models")
        .compose(
            request ->
                request
                    .putHeader(HttpHeaders.AUTHORIZATION, "Bearer " + apiKey)
                    .putHeader(HttpHeaders.USER_AGENT, Constants.USER_AGENT)
                    .putHeader(HttpHeaders.ACCEPT, HttpHeaderValues.APPLICATION_JSON)
                    .send())
        .expecting(HttpResponseExpectation.SC_OK)
        .mapEmpty();
  }

  private record BackendHealth(ProviderHealthStatus status, Optional<String> error) {

    private static BackendHealth starting() {
      return new BackendHealth(
          ProviderHealthStatus.PROVIDER_HEALTH_STATUS_STARTING, Optional.empty());
    }

    private static BackendHealth running() {
      return new BackendHealth(
          ProviderHealthStatus.PROVIDER_HEALTH_STATUS_RUNNING, Optional.empty());
    }

    private static BackendHealth unhealthy(final String error) {
      return new BackendHealth(
          ProviderHealthStatus.PROVIDER_HEALTH_STATUS_UNHEALTHY, Optional.of(error));
    }
  }
}
