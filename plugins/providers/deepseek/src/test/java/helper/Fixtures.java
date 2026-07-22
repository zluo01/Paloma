package helper;

import io.vertx.core.buffer.Buffer;
import io.vertx.core.json.JsonObject;
import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Objects;

public final class Fixtures {
  public static final String RESPONSE = "response";
  public static final String RESPONSE_W_TOOLCALL = "response_w_toolcall";
  public static final String RESPONSE_W_MULTI_TOOLCALLS = "response_w_multi_toolcalls";

  private static final String DATA_PREFIX = "data:";
  private static final String DONE_SIGNAL = "[DONE]";

  private Fixtures() {}

  public static List<JsonObject> chunks(final String name) throws IOException {
    return lines(name).stream()
        .filter(line -> line.startsWith(DATA_PREFIX))
        .map(line -> line.substring(DATA_PREFIX.length()).strip())
        .filter(payload -> !payload.equals(DONE_SIGNAL))
        .map(JsonObject::new)
        .toList();
  }

  private static List<String> lines(final String name) throws IOException {
    try (var reader =
        new BufferedReader(
            new InputStreamReader(
                Objects.requireNonNull(Fixtures.class.getResourceAsStream("/fixtures/" + name)),
                StandardCharsets.UTF_8))) {
      return reader.lines().toList();
    }
  }

  public static Buffer raw(final String name) throws IOException {
    try (var in = Objects.requireNonNull(Fixtures.class.getResourceAsStream("/fixtures/" + name))) {
      return Buffer.buffer(in.readAllBytes());
    }
  }

  public static List<JsonObject> deltas(final String name) throws IOException {
    return chunks(name).stream()
        .map(chunk -> chunk.getJsonArray("choices").getJsonObject(0).getJsonObject("delta"))
        .toList();
  }
}
