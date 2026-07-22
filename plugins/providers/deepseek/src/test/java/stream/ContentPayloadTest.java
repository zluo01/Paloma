package stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.github.zluo01.scry.proto.v1.ChatResponse.PayloadCase;
import constants.Constants;
import helper.CollectingEmitter;
import helper.Fixtures;
import io.vertx.core.json.JsonObject;
import java.io.IOException;
import java.util.List;
import org.junit.jupiter.api.Test;

class ContentPayloadTest {
  private static final String FIXTURE = "response";
  private static final String CONTENT_KEY = "content";
  private static final String REASON_KEY = "reasoning_content";

  @Test
  void streamsAndFlushesRealResponse() throws IOException {
    final List<JsonObject> deltas = Fixtures.deltas(FIXTURE);
    final CollectingEmitter emitter = new CollectingEmitter();
    final ContentPayload payload = new ContentPayload(deltas.getFirst());

    deltas.stream().skip(1).forEach(delta -> payload.accumulate(delta, emitter));

    final var expectedReasoning =
        deltas.stream()
            .map(delta -> delta.getString(REASON_KEY))
            .filter(value -> value != null && !value.isEmpty())
            .toList();
    final var expectedContent =
        deltas.stream()
            .map(delta -> delta.getString(CONTENT_KEY))
            .filter(value -> value != null && !value.isEmpty())
            .toList();
    final var events = emitter.events();
    final var streamed = expectedReasoning.size() + expectedContent.size();

    assertEquals(streamed, events.size());

    for (var i = 0; i < expectedReasoning.size(); i++) {
      final var event = events.get(i);
      assertEquals(PayloadCase.REASONING_DELTA, event.getPayloadCase());
      assertEquals(expectedReasoning.get(i), event.getReasoningDelta());
    }

    for (var i = 0; i < expectedContent.size(); i++) {
      final var event = events.get(expectedReasoning.size() + i);
      assertEquals(PayloadCase.TEXT_DELTA, event.getPayloadCase());
      assertEquals(expectedContent.get(i), event.getTextDelta().getDelta());
      assertEquals(Constants.PROVIDER_ID, event.getTextDelta().getProviderId());
      assertEquals(Constants.BACKEND_ID, event.getTextDelta().getBackendId());
    }

    payload.flush(emitter);
    assertEquals(streamed + 2, events.size());

    final var reasoningItem = events.get(streamed).getOutputItem();
    assertTrue(reasoningItem.hasReasoning());
    assertEquals(1, reasoningItem.getReasoning().getReasoningCount());
    assertEquals(
        String.join("", expectedReasoning),
        reasoningItem.getReasoning().getReasoning(0).getContent());

    final var messageItem = events.get(streamed + 1).getOutputItem();
    assertTrue(messageItem.hasMessage());
    assertEquals(1, messageItem.getMessage().getMessageCount());
    assertEquals(
        String.join("", expectedContent), messageItem.getMessage().getMessage(0).getContent());
  }
}
