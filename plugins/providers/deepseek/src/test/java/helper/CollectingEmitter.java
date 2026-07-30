package helper;

import com.github.zluo01.paloma.proto.v1.ChatResponse;
import java.util.ArrayList;
import java.util.List;
import java.util.function.Consumer;
import stream.ChatEventEmitter;

public final class CollectingEmitter implements ChatEventEmitter {
  private final List<ChatResponse> events = new ArrayList<>();

  @Override
  public void emit(final Consumer<ChatResponse.Builder> payload) {
    final var builder = ChatResponse.newBuilder();
    payload.accept(builder);
    events.add(builder.build());
  }

  public List<ChatResponse> events() {
    return events;
  }
}
