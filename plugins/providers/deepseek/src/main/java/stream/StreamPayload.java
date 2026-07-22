package stream;

import io.vertx.core.json.JsonObject;

public interface StreamPayload {
  StreamMessageType type();

  void accumulate(JsonObject payload, ChatEventEmitter emitter);

  void flush(ChatEventEmitter emitter);
}
