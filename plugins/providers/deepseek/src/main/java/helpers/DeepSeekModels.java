package helpers;

import static constants.Constants.PROVIDER_ID;

import com.github.zluo01.paloma.proto.v1.Model;
import entities.DeepseekModel;
import io.vertx.core.buffer.Buffer;
import io.vertx.core.json.JsonArray;
import io.vertx.core.json.JsonObject;
import java.io.IOException;
import java.io.InputStream;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

public class DeepSeekModels {
  private final Map<String, DeepseekModel> models;

  public DeepSeekModels() {
    this.models = load();
  }

  public List<Model> models() {
    return models.values().stream().map(DeepseekModel::model).toList();
  }

  public boolean supportImages(final String modelId) {
    if (!models.containsKey(modelId)) {
      return false;
    }
    return models.get(modelId).supportImage();
  }

  private static Map<String, DeepseekModel> load() {
    try (InputStream models = DeepSeekModels.class.getResourceAsStream("/models.json")) {
      Objects.requireNonNull(models, "Fail to find models.json.");

      final Map<String, DeepseekModel> parsed = new LinkedHashMap<>();
      for (final Object entry : new JsonArray(Buffer.buffer(models.readAllBytes()))) {
        final JsonObject node = (JsonObject) entry;
        final String id = node.getString("id");
        final JsonObject reasoning = node.getJsonObject("reasoning");
        final boolean supportImage = node.getJsonArray("input_modalities").contains("image");
        final Model.Builder builder =
            Model.newBuilder()
                .setId(id)
                .setName(node.getString("name"))
                .setProvider(PROVIDER_ID)
                .setDefaultReasoningEffort(reasoning.getString("default_effort"));
        reasoning
            .getJsonArray("supported_efforts")
            .forEach(effort -> builder.addSupportedReasoningEfforts((String) effort));
        parsed.computeIfAbsent(id, _ -> new DeepseekModel(builder.build(), supportImage));
      }
      return Collections.unmodifiableMap(parsed);
    } catch (IOException e) {
      throw new ExceptionInInitializerError(e);
    }
  }
}
