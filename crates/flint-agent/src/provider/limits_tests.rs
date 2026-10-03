use pretty_assertions::assert_eq;
use serde_json::json;

use super::ModelLimits;
use super::model_limits_from_listing;
use crate::session::budget_for_window;

#[test]
fn reads_openrouter_style_limits_for_the_configured_model() {
    // Shape of the Surplus / OpenRouter `GET /models` listing.
    let listing = json!({"data": [
        {"id": "deepseek-v4-flash", "context_length": 1_000_000,
         "top_provider": {"context_length": 1_000_000, "max_completion_tokens": 128_000}},
        {"id": "deepseek-v4.1-flash", "context_length": 1_048_576,
         "top_provider": {"context_length": 1_048_576, "max_completion_tokens": 384_000}},
    ]});
    assert_eq!(
        model_limits_from_listing(&listing, "deepseek-v4.1-flash"),
        Some(ModelLimits {
            context_window: 1_048_576,
            max_output: Some(384_000)
        })
    );
    assert_eq!(model_limits_from_listing(&listing, "unknown"), None);
}

#[test]
fn reads_vllm_and_bare_listings() {
    let vllm = json!({"data": [{"id": "qwen", "max_model_len": 32_768}]});
    assert_eq!(
        model_limits_from_listing(&vllm, "qwen"),
        Some(ModelLimits {
            context_window: 32_768,
            max_output: None
        })
    );
    // OpenAI's own listing has no limits.
    let openai = json!({"data": [{"id": "gpt-4.1-mini", "object": "model"}]});
    assert_eq!(model_limits_from_listing(&openai, "gpt-4.1-mini"), None);
}

#[test]
fn budget_is_the_window_minus_a_bounded_reply_reserve() {
    let deepseek = ModelLimits {
        context_window: 1_048_576,
        max_output: Some(384_000),
    };
    assert_eq!(budget_for_window(Some(deepseek)), 1_048_576 - 64_000);
    let small = ModelLimits {
        context_window: 32_768,
        max_output: None,
    };
    assert_eq!(budget_for_window(Some(small)), 16_384);
    assert_eq!(budget_for_window(None), 112_000);
}
