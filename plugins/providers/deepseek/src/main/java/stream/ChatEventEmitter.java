package stream;

import com.github.zluo01.paloma.proto.v1.ChatResponse;
import java.util.function.Consumer;

@FunctionalInterface
public interface ChatEventEmitter {
  void emit(Consumer<ChatResponse.Builder> payload);
}
