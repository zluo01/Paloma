use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    sync::LazyLock,
};

use aws_sdk_bedrock::types::{
    FoundationModelLifecycleStatus, FoundationModelSummary, InferenceProfileType, InferenceType,
    ModelModality,
};
use aws_smithy_types::error::display::DisplayErrorContext;
use paloma_provider_base::{ProviderError, Result};
use paloma_provider_protocol::v1::Model;

static CONVERSE_UNSUPPORTED: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["twelvelabs.pegasus"]));

/// have to hard code as no API return such information
static ADAPTIVE_THINKING_XHIGH: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "anthropic.claude-fable-5-1",
        "anthropic.claude-fable-5",
        "anthropic.claude-opus-5",
        "anthropic.claude-sonnet-5",
        "anthropic.claude-opus-4-8",
        "anthropic.claude-opus-4-7",
    ])
});

/// have to hard code as no API return such information
static ADAPTIVE_THINKING: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["anthropic.claude-opus-4-6", "anthropic.claude-sonnet-4-6"]));

/// have to hard code as no API return such information
static THINKING: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "anthropic.claude-opus-4-5",
        "anthropic.claude-sonnet-4-5",
        "anthropic.claude-haiku-4-5",
    ])
});

const ADAPTIVE_XHIGH_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const ADAPTIVE_EFFORTS: &[&str] = &["low", "medium", "high", "max"];
pub(super) const DEFAULT_EFFORT: &str = "default";
const DEFAULT_EFFORTS: &[&str] = &[DEFAULT_EFFORT];

/// only claude models has effort, all others use default
fn efforts_for(model_id: &str) -> &'static [&'static str] {
    if ADAPTIVE_THINKING_XHIGH.iter().any(|f| model_id.contains(f)) {
        ADAPTIVE_XHIGH_EFFORTS
    } else if ADAPTIVE_THINKING.iter().any(|f| model_id.contains(f)) {
        ADAPTIVE_EFFORTS
    } else {
        DEFAULT_EFFORTS
    }
}

pub(super) fn supports_thinking(model_id: &str) -> bool {
    THINKING.iter().any(|f| model_id.contains(f))
}

pub(super) async fn fetch_models(client: &aws_sdk_bedrock::Client) -> Result<Vec<Model>> {
    let (on_demands, inferences) = models(client).await?;

    let inference_ids: HashSet<&str> = inferences.iter().map(|o| o.model_id()).collect();
    let inference_maps = inference_profiles(client, &inference_ids).await?;

    let on_demands = on_demands
        .iter()
        .map(|summary| to_model(summary, summary.model_id()));
    let inferences = inferences.iter().filter_map(|summary| {
        inference_maps
            .get(summary.model_id())
            .map(|profile_id| to_model(summary, profile_id))
    });

    let mut models: Vec<Model> = on_demands.chain(inferences).collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

fn to_model(summary: &FoundationModelSummary, id: &str) -> Model {
    let efforts: Vec<String> = efforts_for(summary.model_id())
        .iter()
        .map(ToString::to_string)
        .collect();
    Model {
        id: id.to_string(),
        name: format!(
            "{} {}",
            summary.provider_name().unwrap_or_default(),
            summary.model_name().unwrap_or_default()
        ),
        // Bedrock's documented default for adaptive thinking
        default_reasoning_effort: if efforts.len() > 1 {
            "high".to_string()
        } else {
            DEFAULT_EFFORT.to_string()
        },
        supported_reasoning_efforts: efforts,
    }
}

/// Returns `(on_demand, inference_profile)`
async fn models(
    client: &aws_sdk_bedrock::Client,
) -> Result<(Vec<FoundationModelSummary>, Vec<FoundationModelSummary>)> {
    let foundation = client
        .list_foundation_models()
        .by_output_modality(ModelModality::Text)
        .send()
        .await
        .map_err(|e| ProviderError::Other(format!("{}", DisplayErrorContext(&e))))?;

    Ok(foundation
        .model_summaries
        .unwrap_or_default()
        .into_iter()
        .filter(|summary| {
            summary
                .model_lifecycle()
                .is_none_or(|m| *m.status() == FoundationModelLifecycleStatus::Active)
        })
        .filter(|summary| !summary.inference_types_supported().is_empty())
        .filter(|summary| summary.response_streaming_supported().unwrap_or(false))
        .filter(|summary| {
            !CONVERSE_UNSUPPORTED
                .iter()
                .any(|f| summary.model_id().contains(f))
        })
        .partition(|summary| {
            summary
                .inference_types_supported()
                .contains(&InferenceType::OnDemand)
        }))
}

/// get inference profiles id for inference profile models
async fn inference_profiles(
    client: &aws_sdk_bedrock::Client,
    inference_ids: &HashSet<&str>,
) -> Result<HashMap<String, String>> {
    let mut profiles: HashMap<String, String> = HashMap::new();
    let mut pages = client
        .list_inference_profiles()
        .type_equals(InferenceProfileType::SystemDefined)
        .max_results(1000)
        .into_paginator()
        .send();
    while let Some(page) = pages.next().await {
        let page =
            page.map_err(|e| ProviderError::Other(format!("{}", DisplayErrorContext(&e))))?;
        for profile in page.inference_profile_summaries() {
            let profile_id = profile.inference_profile_id();
            // model arn: arn:aws:bedrock:<region>::foundation-model/<model id>
            let model_ids = profile
                .models()
                .iter()
                .filter_map(|m| m.model_arn())
                .filter_map(|arn| arn.rsplit('/').next())
                .filter(|model_id| inference_ids.contains(model_id));
            for model_id in model_ids {
                match profiles.entry(model_id.to_string()) {
                    // favor the regional over global
                    Entry::Occupied(mut existing) if existing.get().starts_with("global.") => {
                        existing.insert(profile_id.to_string());
                    },
                    Entry::Occupied(_) => {},
                    Entry::Vacant(vacant) => {
                        vacant.insert(profile_id.to_string());
                    },
                }
            }
        }
    }
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_efforts_lookup() {
        for id in [
            "us.anthropic.claude-fable-5-1",
            "us.anthropic.claude-fable-5",
            "us.anthropic.claude-opus-5",
            "us.anthropic.claude-sonnet-5",
            "us.anthropic.claude-opus-4-8",
            "us.anthropic.claude-opus-4-7",
        ] {
            assert_eq!(efforts_for(id), ADAPTIVE_XHIGH_EFFORTS, "{id}");
        }
        for id in [
            "us.anthropic.claude-opus-4-6-v1",
            "us.anthropic.claude-sonnet-4-6",
        ] {
            assert_eq!(efforts_for(id), ADAPTIVE_EFFORTS, "{id}");
        }
        for id in [
            "us.anthropic.claude-haiku-4-5-20251001-v1:0",
            "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            "us.anthropic.claude-opus-4-5-20251101-v1:0",
            "deepseek.v3.2",
            "us.deepseek.r1-v1:0",
            "us.openai.gpt-5.6-terra",
            "amazon.nova-lite-v1:0",
        ] {
            assert_eq!(efforts_for(id), DEFAULT_EFFORTS, "{id}");
        }
    }
}
