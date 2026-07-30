package stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.fail;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.when;

import com.github.zluo01.paloma.proto.v1.ChatResponse.PayloadCase;
import com.github.zluo01.paloma.proto.v1.ResponseEvent;
import enums.MessageEvent;
import helper.Fixtures;
import helper.MockSseVerticle;
import io.vertx.core.Vertx;
import io.vertx.core.eventbus.EventBus;
import io.vertx.core.http.HttpClient;
import io.vertx.core.http.HttpClientOptions;
import io.vertx.core.http.HttpVersion;
import io.vertx.core.json.JsonObject;
import io.vertx.junit5.VertxExtension;
import io.vertx.junit5.VertxTestContext;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.function.Consumer;
import java.util.stream.Collectors;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.mockito.Mock;
import org.mockito.junit.jupiter.MockitoExtension;

@ExtendWith(VertxExtension.class)
@ExtendWith(MockitoExtension.class)
class ChatStreamTest {
  private static final long EVENT_ID = 42L;

  private static HttpClient client;

  @Mock private EventBus eventBus;

  private List<ResponseEvent> events;
  private Consumer<ResponseEvent> onEvent;

  @BeforeAll
  static void deployMockSse(final Vertx vertx, final VertxTestContext context) throws Throwable {
    final var verticle = new MockSseVerticle();
    vertx.deployVerticle(verticle).onComplete(context.succeedingThenComplete());

    assertTrue(context.awaitCompletion(5, TimeUnit.SECONDS));
    if (context.failed()) {
      throw context.causeOfFailure();
    }

    client =
        vertx.createHttpClient(
            new HttpClientOptions()
                .setDefaultHost("localhost")
                .setDefaultPort(verticle.port())
                .setProtocolVersion(HttpVersion.HTTP_2)
                .setHttp2ClearTextUpgrade(false));
  }

  @BeforeEach
  void setUp() {
    events = new ArrayList<>();
    onEvent = ignored -> {};
    when(eventBus.send(eq(MessageEvent.RESPONSE_EVENT.name()), any()))
        .thenAnswer(
            invocation -> {
              final ResponseEvent event = invocation.getArgument(1);
              events.add(event);
              onEvent.accept(event);
              return eventBus;
            });
  }

  @Test
  void streamsContentResponse(final Vertx vertx) throws InterruptedException {
    final var closed = new CountDownLatch(1);
    chatStream(vertx, closed)
        .start(new JsonObject().put(MockSseVerticle.FIXTURE, Fixtures.RESPONSE));
    awaitClose(closed);

    events.forEach(event -> assertEquals(EVENT_ID, event.getEventId()));

    final var size = events.size();
    assertEquals(PayloadCase.DONE, events.getLast().getChatResponse().getPayloadCase());

    final var reasoningItem = events.get(size - 3).getChatResponse().getOutputItem();
    assertTrue(reasoningItem.hasReasoning());
    final var messageItem = events.get(size - 2).getChatResponse().getOutputItem();
    assertTrue(messageItem.hasMessage());

    final var deltas = events.subList(0, size - 3);
    assertReasoningThenTextDeltas(deltas);

    assertEquals(
        joinedDeltas(deltas, PayloadCase.REASONING_DELTA),
        reasoningItem.getReasoning().getReasoning(0).getContent());
    assertEquals(
        joinedDeltas(deltas, PayloadCase.TEXT_DELTA),
        messageItem.getMessage().getMessage(0).getContent());
  }

  @Test
  void streamsToolCallResponse(final Vertx vertx) throws InterruptedException {
    final var closed = new CountDownLatch(1);
    chatStream(vertx, closed)
        .start(new JsonObject().put(MockSseVerticle.FIXTURE, Fixtures.RESPONSE_W_TOOLCALL));
    awaitClose(closed);

    final var size = events.size();
    assertEquals(PayloadCase.DONE, events.getLast().getChatResponse().getPayloadCase());

    final var toolCallItem = events.get(size - 2).getChatResponse().getOutputItem();
    assertTrue(toolCallItem.hasToolCall());
    assertEquals("call_00_SmYiIBb9lwglCJzAoCyD9731", toolCallItem.getToolCall().getCallId());
    assertEquals("mcp__scryfall__search_cards", toolCallItem.getToolCall().getName());
    assertEquals(
        "{\"query\": \"c:u (t:instant or t:sorcery) mana>=0 phyrexian\"}",
        toolCallItem.getToolCall().getArguments());

    assertTrue(events.get(size - 4).getChatResponse().getOutputItem().hasReasoning());
    assertTrue(events.get(size - 3).getChatResponse().getOutputItem().hasMessage());

    assertReasoningThenTextDeltas(events.subList(0, size - 4));
  }

  @Test
  void streamsMultiToolCallResponse(final Vertx vertx) throws InterruptedException {
    final var closed = new CountDownLatch(1);
    chatStream(vertx, closed)
        .start(new JsonObject().put(MockSseVerticle.FIXTURE, Fixtures.RESPONSE_W_MULTI_TOOLCALLS));
    awaitClose(closed);

    final var size = events.size();
    assertEquals(PayloadCase.DONE, events.getLast().getChatResponse().getPayloadCase());

    final var expectedCalls =
        List.of(
            Map.entry("call_00_xAz8cMMZEJQbjVaS8Rz99204", "{\"name\": \"Pact of Negation\"}"),
            Map.entry("call_01_dEXJ4ybYkyLcrxz61xYc0880", "{\"name\": \"Ancestral Vision\"}"),
            Map.entry(
                "call_02_Qm3xVb9sLp1KdT7uWzHn4402", "{\"name\": \"Rograkh, Son of Rohgahh\"}"),
            Map.entry(
                "call_03_Ye5cRj2mNv8sBq0aXk6f7713",
                "{\"name\": \"Asmoranomardicadaistinaculdacar\"}"),
            Map.entry("call_04_Hf1uPw4dGz6yTl3oCs9e2211", "{\"name\": \"Force of Will\"}"));

    final var firstCall = size - 1 - expectedCalls.size();
    for (var i = 0; i < expectedCalls.size(); i++) {
      final var item = events.get(firstCall + i).getChatResponse().getOutputItem();
      assertTrue(item.hasToolCall());
      assertEquals(expectedCalls.get(i).getKey(), item.getToolCall().getCallId());
      assertEquals("mcp__scryfall__get_prices_by_name", item.getToolCall().getName());
      assertEquals(expectedCalls.get(i).getValue(), item.getToolCall().getArguments());
    }

    assertTrue(events.get(firstCall - 2).getChatResponse().getOutputItem().hasReasoning());
    final var messageItem = events.get(firstCall - 1).getChatResponse().getOutputItem();
    assertTrue(messageItem.hasMessage());
    assertTrue(messageItem.getMessage().getMessage(0).getContent().startsWith("Sure,"));

    assertReasoningThenTextDeltas(events.subList(0, firstCall - 2));
  }

  @Test
  void emitsErrorForNonOkStatus(final Vertx vertx) throws InterruptedException {
    final var error =
        new JsonObject(
            """
            {"error":{"message":"Authentication Fails (no such user)","type":"authentication_error","param":null,"code":"invalid_request_error"}}
            """);
    final var closed = new CountDownLatch(1);
    chatStream(vertx, closed)
        .start(new JsonObject().put(MockSseVerticle.STATUS, 401).put(MockSseVerticle.BODY, error));
    awaitClose(closed);

    assertEquals(1, events.size());
    assertEquals(
        "Authentication Fails (no such user)", events.getFirst().getChatResponse().getError());
  }

  @Test
  void emitsErrorWhenTextDeltaHandlingThrows(final Vertx vertx) throws InterruptedException {
    final var closed = new CountDownLatch(1);
    onEvent =
        event -> {
          if (event.getChatResponse().getPayloadCase() == PayloadCase.TEXT_DELTA) {
            throw new IllegalStateException("boom");
          }
        };
    chatStream(vertx, closed)
        .start(new JsonObject().put(MockSseVerticle.FIXTURE, Fixtures.RESPONSE));
    awaitClose(closed);

    final var last = events.getLast().getChatResponse();
    assertEquals(PayloadCase.ERROR, last.getPayloadCase());
    assertEquals("SSE parse error: boom", last.getError());

    // the failing text delta is recorded before the handler throws; nothing streams after it
    assertReasoningThenTextDeltas(events.subList(0, events.size() - 1));
    assertEquals(
        1,
        events.stream()
            .filter(event -> event.getChatResponse().getPayloadCase() == PayloadCase.TEXT_DELTA)
            .count());
  }

  @Test
  void abortEmitsErrorAndCloses(final Vertx vertx) throws InterruptedException {
    final var closed = new CountDownLatch(1);
    final var stream = chatStream(vertx, closed);
    onEvent =
        event -> {
          switch (event.getChatResponse().getPayloadCase()) {
            case REASONING_DELTA -> stream.abort("client cancelled");
            case TEXT_DELTA -> fail("text delta emitted after abort");
            default -> {}
          }
        };
    stream.start(new JsonObject().put(MockSseVerticle.FIXTURE, Fixtures.RESPONSE));
    awaitClose(closed);

    assertEquals(2, events.size());
    assertEquals("client cancelled", events.getLast().getChatResponse().getError());
  }

  private ChatStream chatStream(final Vertx vertx, final CountDownLatch closed) {
    return new ChatStream(
        vertx, eventBus, client, EVENT_ID, "test-key", ignored -> closed.countDown());
  }

  private static void awaitClose(final CountDownLatch closed) throws InterruptedException {
    assertTrue(closed.await(10, TimeUnit.SECONDS), "stream did not close in time");
  }

  /** All reasoning deltas stream first, then all text deltas, with no other event in between. */
  private static void assertReasoningThenTextDeltas(final List<ResponseEvent> deltas) {
    final var reasoningCount =
        deltas.stream()
            .filter(
                event -> event.getChatResponse().getPayloadCase() == PayloadCase.REASONING_DELTA)
            .count();
    for (var i = 0; i < deltas.size(); i++) {
      assertEquals(
          i < reasoningCount ? PayloadCase.REASONING_DELTA : PayloadCase.TEXT_DELTA,
          deltas.get(i).getChatResponse().getPayloadCase());
    }
  }

  private static String joinedDeltas(final List<ResponseEvent> deltas, final PayloadCase kind) {
    return deltas.stream()
        .filter(event -> event.getChatResponse().getPayloadCase() == kind)
        .map(
            event ->
                kind == PayloadCase.REASONING_DELTA
                    ? event.getChatResponse().getReasoningDelta()
                    : event.getChatResponse().getTextDelta().getDelta())
        .collect(Collectors.joining());
  }
}
