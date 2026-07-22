package stream;

import com.github.zluo01.scry.proto.v1.ConversationItem;
import com.github.zluo01.scry.proto.v1.ConversationMessage;
import com.github.zluo01.scry.proto.v1.MessageContentItem;
import com.github.zluo01.scry.proto.v1.Reasoning;
import com.github.zluo01.scry.proto.v1.SummaryItem;
import com.github.zluo01.scry.proto.v1.TextDelta;
import constants.Constants;
import io.vertx.core.json.JsonObject;
import java.util.Optional;

public class ContentPayload implements StreamPayload {
  private static final String CONTENT_KEY = "content";
  private static final String REASON_KEY = "reasoning_content";

  private final StringBuilder content;
  private final StringBuilder reasoning;

  public ContentPayload(final JsonObject payload) {
    this.content =
        new StringBuilder(Optional.ofNullable(payload.getString(CONTENT_KEY)).orElse(""));
    this.reasoning =
        new StringBuilder(Optional.ofNullable(payload.getString(REASON_KEY)).orElse(""));
  }

  @Override
  public StreamMessageType type() {
    return StreamMessageType.CONTENT;
  }

  @Override
  public void accumulate(final JsonObject payload, final ChatEventEmitter emitter) {
    final var contentDelta = payload.getString(CONTENT_KEY);
    if (contentDelta != null && !contentDelta.isEmpty()) {
      content.append(contentDelta);
      emitter.emit(
          c ->
              c.setTextDelta(
                  TextDelta.newBuilder()
                      .setProviderId(Constants.PROVIDER_ID)
                      .setBackendId(Constants.BACKEND_ID)
                      .setDelta(contentDelta)));
    }

    final var reasoningDelta = payload.getString(REASON_KEY);
    if (reasoningDelta != null && !reasoningDelta.isEmpty()) {
      reasoning.append(reasoningDelta);
      emitter.emit(c -> c.setReasoningDelta(reasoningDelta));
    }
  }

  @Override
  public void flush(final ChatEventEmitter emitter) {
    if (!reasoning.isEmpty()) {
      emitter.emit(
          c ->
              c.setOutputItem(
                  ConversationItem.newBuilder()
                      .setReasoning(
                          Reasoning.newBuilder()
                              .addReasoning(
                                  SummaryItem.newBuilder().setContent(reasoning.toString()).build())
                              .build())));
    }

    if (!content.isEmpty()) {
      emitter.emit(
          c ->
              c.setOutputItem(
                  ConversationItem.newBuilder()
                      .setMessage(
                          ConversationMessage.newBuilder()
                              .addMessage(
                                  MessageContentItem.newBuilder()
                                      .setContent(content.toString())
                                      .build())
                              .build())));
    }
  }
}
