package entities;

import com.github.zluo01.paloma.proto.v1.Model;

public record DeepseekModel(Model model, boolean supportImage) {}
