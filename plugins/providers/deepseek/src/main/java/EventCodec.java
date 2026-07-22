import io.vertx.core.buffer.Buffer;
import io.vertx.core.eventbus.MessageCodec;

public final class EventCodec<T> implements MessageCodec<T, T> {
  private final Class<T> type;

  public EventCodec(final Class<T> type) {
    this.type = type;
  }

  @Override
  public void encodeToWire(final Buffer buffer, final T t) {
    throw new UnsupportedOperationException("Codec is only for local usage");
  }

  @Override
  public T decodeFromWire(final int pos, final Buffer buffer) {
    throw new UnsupportedOperationException("Codec is only for local usage");
  }

  @Override
  public T transform(final T msg) {
    return msg;
  }

  @Override
  public String name() {
    return type.getName();
  }

  @Override
  public byte systemCodecID() {
    return -1;
  }
}
