import com.github.zluo01.scry.proto.v1.ChatRequest;
import com.github.zluo01.scry.proto.v1.ChatRequestMessage;
import com.github.zluo01.scry.proto.v1.ConversationItem;
import com.github.zluo01.scry.proto.v1.ToolDefinition;
import constants.Constants;
import io.vertx.core.json.Json;
import io.vertx.core.json.JsonArray;
import io.vertx.core.json.JsonObject;
import java.io.StringWriter;
import java.util.Map;
import java.util.TreeMap;
import java.util.logging.Logger;
import javax.xml.stream.XMLOutputFactory;
import javax.xml.stream.XMLStreamException;

final class DeepSeekCodec {
  private static final Logger LOGGER = Logger.getLogger(DeepSeekCodec.class.getSimpleName());

  private static final String ENVIRONMENT_CONTEXT = buildEnvironmentContext();

  private DeepSeekCodec() {}

  static JsonObject buildRequestBody(final ChatRequest request) {
    final var messages =
        new JsonArray()
            .add(new JsonObject().put("role", "system").put("content", request.getInstruction()))
            .add(new JsonObject().put("role", "system").put("content", ENVIRONMENT_CONTEXT));
    for (final ChatRequestMessage message : request.getMessagesList()) {
      if (!message.hasItem()) {
        continue;
      }
      final var encoded =
          encodeConversationItem(
              message.getItem(), Constants.PROVIDER_ID.equals(message.getProviderId()));
      if (encoded != null) {
        messages.add(encoded);
      }
    }

    final var body =
        new JsonObject()
            .put("model", request.getModel())
            .put("messages", messages)
            .put("stream", true)
            .put("thinking", new JsonObject().put("type", "enabled"));
    if (!request.getEffort().isBlank()) {
      body.put("reasoning_effort", request.getEffort());
    }

    final var tools = new JsonArray();
    for (final ToolDefinition tool : request.getToolsList()) {
      try {
        tools.add(
            new JsonObject()
                .put("type", "function")
                .put(
                    "function",
                    new JsonObject()
                        .put("name", tool.getName())
                        .put("description", tool.getDescription())
                        .put("parameters", new JsonObject(tool.getParameters()))));
      } catch (Exception e) {
        LOGGER.warning("tool " + tool.getName() + " has malformed parameters JSON; skipping tool.");
      }
    }
    if (!tools.isEmpty()) {
      body.put("tools", tools);
    }
    return body;
  }

  private static JsonObject encodeConversationItem(
      final ConversationItem item, final boolean sameProvider) {
    return switch (item.getItemCase()) {
      case USER_PROMPT ->
          new JsonObject().put("role", "user").put("content", item.getUserPrompt().getPrompt());
      case MESSAGE -> {
        final var content = new StringBuilder();
        item.getMessage().getMessageList().forEach(m -> content.append(m.getContent()));
        yield new JsonObject().put("role", "assistant").put("content", content.toString());
      }
      case TOOL_CALL -> {
        final var call = item.getToolCall();
        // Same-provider meta restores reasoning_content onto the assistant
        // tool-call message; DeepSeek rejects tool-call turns without it.
        yield metaToObject(call.getProviderMetaMap(), sameProvider)
            .put("role", "assistant")
            .put(
                "tool_calls",
                new JsonArray()
                    .add(
                        new JsonObject()
                            .put("id", call.getCallId())
                            .put("type", "function")
                            .put(
                                "function",
                                new JsonObject()
                                    .put("name", call.getName())
                                    .put("arguments", call.getArguments()))));
      }
      case TOOL_RESULT -> {
        final var result = item.getToolResult();
        yield new JsonObject()
            .put("role", "tool")
            .put("tool_call_id", result.getCallId())
            .put("content", result.getOutput());
      }
      case REASONING, HOSTED_TOOL, UNKNOWN, ITEM_NOT_SET -> null;
    };
  }

  private static JsonObject metaToObject(final Map<String, String> meta, final boolean include) {
    final var object = new JsonObject();
    if (!include) {
      return object;
    }
    meta.forEach(
        (key, value) -> {
          try {
            object.put(key, Json.decodeValue(value));
          } catch (Exception e) {
            LOGGER.warning("skipping malformed provider meta entry " + key);
          }
        });
    return object;
  }

  private static String buildEnvironmentContext() {
    final var os = System.getProperty("os.name", "unknown").toLowerCase();
    final var envs = new TreeMap<String, String>();
    envs.put("os", os);
    envs.put("os_family", os.contains("windows") ? "windows" : "unix");
    envs.put("arch", System.getProperty("os.arch", "unknown"));
    envs.put("home", System.getProperty("user.home", "unknown"));
    envs.put("shell", System.getenv().getOrDefault("SHELL", "unknown"));
    try {
      final var out = new StringWriter();
      final var writer = XMLOutputFactory.newDefaultFactory().createXMLStreamWriter(out);
      writer.writeStartElement("environment_context");
      for (final var entry : envs.entrySet()) {
        writer.writeStartElement(entry.getKey());
        writer.writeCharacters(entry.getValue());
        writer.writeEndElement();
      }
      writer.writeEndElement();
      writer.close();
      return out.toString();
    } catch (XMLStreamException e) {
      throw new ExceptionInInitializerError(e);
    }
  }
}
