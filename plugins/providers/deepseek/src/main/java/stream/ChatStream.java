package stream;

import com.github.zluo01.paloma.proto.v1.ChatResponse;
import com.github.zluo01.paloma.proto.v1.Done;
import com.github.zluo01.paloma.proto.v1.ResponseEvent;
import constants.Constants;
import enums.MessageEvent;
import io.netty.handler.codec.http.HttpHeaderValues;
import io.netty.handler.codec.http.HttpResponseStatus;
import io.vertx.core.Vertx;
import io.vertx.core.buffer.Buffer;
import io.vertx.core.eventbus.EventBus;
import io.vertx.core.http.HttpClient;
import io.vertx.core.http.HttpClientRequest;
import io.vertx.core.http.HttpClientResponse;
import io.vertx.core.http.HttpHeaders;
import io.vertx.core.http.HttpMethod;
import io.vertx.core.json.JsonObject;
import io.vertx.core.parsetools.RecordParser;
import java.time.Duration;
import java.util.function.Consumer;

public final class ChatStream {
  private static final Duration SSE_IDLE_TIMEOUT = Duration.ofMinutes(5);

  private static final String DATA_PREFIX = "data:";
  private static final String DONE_SIGNAL = "[DONE]";

  private final Vertx vertx;
  private final EventBus eventBus;
  private final HttpClient httpClient;
  private final long eventId;
  private final String apiKey;
  private final Consumer<ChatStream> onClosed;

  private StreamPayload placeholder;
  private HttpClientRequest request;
  private long timerId = -1;
  private boolean finished;

  public ChatStream(
      final Vertx vertx,
      final EventBus eventBus,
      final HttpClient httpClient,
      final long eventId,
      final String apiKey,
      final Consumer<ChatStream> onClosed) {
    this.vertx = vertx;
    this.eventBus = eventBus;
    this.httpClient = httpClient;
    this.eventId = eventId;
    this.apiKey = apiKey;
    this.onClosed = onClosed;
  }

  public void start(final JsonObject body) {
    resetIdleTimeout();
    httpClient
        .request(HttpMethod.POST, "/chat/completions")
        .compose(
            req -> {
              request = req;
              return req.putHeader(HttpHeaders.AUTHORIZATION, "Bearer " + apiKey)
                  .putHeader(HttpHeaders.CONTENT_TYPE, HttpHeaderValues.APPLICATION_JSON)
                  .putHeader(HttpHeaders.USER_AGENT, Constants.USER_AGENT)
                  .putHeader(HttpHeaders.ACCEPT, HttpHeaderValues.TEXT_EVENT_STREAM)
                  .send(body.encode());
            })
        .onSuccess(this::onResponse)
        .onFailure(e -> fail("Fail to connect to " + Constants.BACKEND_ID + ": " + e.getMessage()));
  }

  public void abort(final String message) {
    fail(message);
  }

  private void onResponse(final HttpClientResponse response) {
    if (response.statusCode() != HttpResponseStatus.OK.code()) {
      response
          .body()
          .onSuccess(body -> fail(parseStreamError(response.statusCode(), body)))
          .onFailure(e -> fail("HTTP " + response.statusCode()));
      return;
    }
    final var parser = RecordParser.newDelimited("\n", this::onLine);
    response.handler(parser);
    response.endHandler(v -> fail("SSE stream ended without " + DONE_SIGNAL));
    response.exceptionHandler(e -> fail("SSE transport error: " + e.getMessage()));
  }

  private void onLine(final Buffer line) {
    if (finished) {
      return;
    }
    resetIdleTimeout();
    final String data = line.toString();
    if (!data.startsWith(DATA_PREFIX)) {
      return;
    }
    final var payload = data.substring(DATA_PREFIX.length()).strip();
    if (payload.equals(DONE_SIGNAL)) {
      complete();
      return;
    }
    try {
      handleChunk(new JsonObject(payload));
    } catch (Exception e) {
      fail("SSE parse error: " + e.getMessage());
    }
  }

  private void handleChunk(final JsonObject chunk) {
    final var choices = chunk.getJsonArray("choices");
    if (choices == null || choices.isEmpty()) {
      return;
    }
    final var choice = choices.getJsonObject(0);
    // the finish_reason chunk carries no payload, only an empty-content placeholder delta
    if (choice.getString("finish_reason") != null) {
      return;
    }
    final var delta = choice.getJsonObject("delta");
    if (delta == null) {
      return;
    }

    // role marks the start of a new response section
    if (delta.getString("role") != null) {
      flushPlaceholder();
      placeholder = new ContentPayload(delta);
      return;
    }

    // tool_calls before content: tool-call deltas may carry a null content key
    if (delta.getJsonArray("tool_calls") != null) {
      final var call = delta.getJsonArray("tool_calls").getJsonObject(0);
      // an opener carries the call id: the previous section (content or call) is complete
      if (call.getString("id") != null) {
        flushPlaceholder();
        placeholder = new ToolCallPayload(call);
      } else {
        if (placeholder == null || placeholder.type() != StreamMessageType.TOOL_CALL) {
          throw new IllegalStateException(
              "tool_calls continuation outside a tool call section: " + delta.encode());
        }
        placeholder.accumulate(call, this::chatEvent);
      }
      return;
    }

    if (delta.getString("content") != null || delta.getString("reasoning_content") != null) {
      if (placeholder == null || placeholder.type() != StreamMessageType.CONTENT) {
        throw new IllegalStateException(
            "content delta arrived outside a content section: " + delta.encode());
      }
      placeholder.accumulate(delta, this::chatEvent);
    }
  }

  private void flushPlaceholder() {
    if (placeholder != null) {
      placeholder.flush(this::chatEvent);
      placeholder = null;
    }
  }

  private void complete() {
    if (finished) {
      return;
    }
    finished = true;
    flushPlaceholder();
    chatEvent(c -> c.setDone(Done.getDefaultInstance()));
    cleanup();
  }

  private void fail(final String message) {
    if (finished) {
      return;
    }
    finished = true;
    chatEvent(c -> c.setError(message));
    cleanup();
  }

  private void cleanup() {
    if (timerId != -1) {
      vertx.cancelTimer(timerId);
      timerId = -1;
    }
    // stop downloading the rest of the response; no-op when already completed
    if (request != null) {
      request.reset();
    }
    onClosed.accept(this);
  }

  private void resetIdleTimeout() {
    if (timerId != -1) {
      vertx.cancelTimer(timerId);
    }
    timerId =
        vertx.setTimer(
            SSE_IDLE_TIMEOUT.toMillis(),
            id -> fail("SSE idle timeout: no activity for " + SSE_IDLE_TIMEOUT.toSeconds() + "s"));
  }

  private void chatEvent(final Consumer<ChatResponse.Builder> payload) {
    final var chat = ChatResponse.newBuilder();
    payload.accept(chat);
    eventBus.send(
        MessageEvent.RESPONSE_EVENT.name(),
        ResponseEvent.newBuilder().setEventId(eventId).setChatResponse(chat).build());
  }

  private String parseStreamError(final int statusCode, final Buffer body) {
    try {
      final var message = new JsonObject(body).getJsonObject("error").getString("message");
      if (message != null && !message.isBlank()) {
        return message;
      }
    } catch (Exception ignored) {
      // fall through to the raw body
    }
    return "HTTP " + statusCode + ": " + body.toString();
  }
}
