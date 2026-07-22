package stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import helper.CollectingEmitter;
import io.vertx.core.json.JsonObject;
import org.junit.jupiter.api.Test;

class ToolCallPayloadTest {

  private static final JsonObject OPENER =
      new JsonObject(
          """
          {"index":0,"id":"call_00_B6YhnSIIoXa6Xq8YhGFT6739","type":"function","function":{"name":"get_weather","arguments":""}}
          """);

  @Test
  void accumulatesArgumentFragmentsAndFlushesToolCall() {
    final var emitter = new CollectingEmitter();
    final var payload = new ToolCallPayload(OPENER);

    assertEquals(StreamMessageType.TOOL_CALL, payload.type());

    payload.accumulate(
        new JsonObject(
            """
            {"index":0,"function":{"arguments":"{\\"location\\": \\"Hang"}}
            """),
        emitter);
    payload.accumulate(
        new JsonObject(
            """
            {"index":0,"function":{"arguments":"zhou\\"}"}}
            """),
        emitter);

    assertTrue(emitter.events().isEmpty());

    payload.flush(emitter);

    assertEquals(1, emitter.events().size());
    final var item = emitter.events().getFirst().getOutputItem();
    assertTrue(item.hasToolCall());
    assertEquals("call_00_B6YhnSIIoXa6Xq8YhGFT6739", item.getToolCall().getCallId());
    assertEquals("get_weather", item.getToolCall().getName());
    assertEquals("{\"location\": \"Hangzhou\"}", item.getToolCall().getArguments());
  }

  @Test
  void rejectsContinuationForMismatchedIndex() {
    final var emitter = new CollectingEmitter();
    final var payload = new ToolCallPayload(OPENER);

    assertThrows(
        IllegalStateException.class,
        () ->
            payload.accumulate(
                new JsonObject(
                    """
                    {"index":1,"function":{"arguments":"{}"}}
                    """),
                emitter));
  }
}
