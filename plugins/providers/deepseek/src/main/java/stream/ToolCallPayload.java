package stream;

import com.github.zluo01.paloma.proto.v1.ConversationItem;
import com.github.zluo01.paloma.proto.v1.ToolCall;
import io.vertx.core.json.JsonObject;
import java.util.Objects;

public class ToolCallPayload implements StreamPayload {

  private final int index;
  private final FunctionCall functionCall;

  public ToolCallPayload(final JsonObject call) {
    this.index =
        Objects.requireNonNull(
            call.getInteger("index"), "missing index in toolcall payload. " + call.encode());
    final var callId =
        Objects.requireNonNull(
            call.getString("id"), "missing id in toolcall payload. " + call.encode());
    final var function =
        Objects.requireNonNull(
            call.getJsonObject("function"),
            "missing function in toolcall payload. " + call.encode());
    final var functionName =
        Objects.requireNonNull(
            function.getString("name"), "missing name in function payload. " + function.encode());
    final var arguments =
        Objects.requireNonNull(
            function.getString("arguments"),
            "missing arguments in function payload. " + function.encode());

    this.functionCall = new FunctionCall(callId, functionName, new StringBuilder(arguments));
  }

  @Override
  public StreamMessageType type() {
    return StreamMessageType.TOOL_CALL;
  }

  @Override
  public void accumulate(final JsonObject call, final ChatEventEmitter emitter) {
    final var callIndex =
        Objects.requireNonNull(
            call.getInteger("index"), "missing index in toolcall payload. " + call.encode());
    if (callIndex != index) {
      throw new IllegalStateException(
          "Get mismatched index "
              + callIndex
              + " for tool call at index "
              + index
              + ": "
              + functionCall
              + ". Payload: "
              + call.encode());
    }
    final var function =
        Objects.requireNonNull(
            call.getJsonObject("function"),
            "missing function in toolcall payload. " + call.encode());
    final var arguments =
        Objects.requireNonNull(
            function.getString("arguments"),
            "missing arguments in function payload. " + function.encode());
    functionCall.args().append(arguments);
  }

  @Override
  public void flush(final ChatEventEmitter emitter) {
    emitter.emit(
        c ->
            c.setOutputItem(
                ConversationItem.newBuilder()
                    .setToolCall(
                        ToolCall.newBuilder()
                            .setCallId(functionCall.id())
                            .setName(functionCall.name())
                            .setArguments(functionCall.args().toString())
                            .build())));
  }

  private record FunctionCall(String id, String name, StringBuilder args) {}
}
