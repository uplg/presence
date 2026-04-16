use anyhow::Result;
use mistralrs::{
    Constraint, IsqBits, ModelBuilder, RequestBuilder, TextMessageRole,
};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

use crate::report::LlmWeekOutput;

const MODEL_ID: &str = "Qwen/Qwen3-0.6B";

pub struct Llm {
    model: Arc<mistralrs::Model>,
}

impl Llm {
    pub async fn load() -> Result<Self> {
        info!("loading {MODEL_ID} with Q4 ISQ...");

        let model = ModelBuilder::new(MODEL_ID)
            .with_auto_isq(IsqBits::Four)
            .with_logging()
            .build()
            .await?;

        info!("model ready");
        Ok(Self {
            model: Arc::new(model),
        })
    }

    /// Generate lunch schedules using JSON schema constraint — guaranteed valid structure.
    pub async fn generate_schedule(
        &self,
        prompt: &str,
        expected_days: usize,
    ) -> Result<LlmWeekOutput> {
        let schema = json!({
            "type": "object",
            "properties": {
                "days": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "lunch_start": {
                                "type": "string",
                                "enum": ["12h00", "12h30", "13h00", "13h30", "14h00"]
                            },
                            "lunch_end": {
                                "type": "string",
                                "enum": ["13h00", "13h30", "14h00", "14h30", "15h00"]
                            }
                        },
                        "required": ["lunch_start", "lunch_end"],
                        "additionalProperties": false
                    },
                    "minItems": expected_days,
                    "maxItems": expected_days
                }
            },
            "required": ["days"],
            "additionalProperties": false
        });

        let request = RequestBuilder::new()
            .set_constraint(Constraint::JsonSchema(schema))
            .set_sampler_max_len(1024)
            .set_sampler_temperature(0.3)
            .add_message(TextMessageRole::User, prompt);

        let response = self.model.send_chat_request(request).await?;

        let raw = response.choices[0]
            .message
            .content
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("empty LLM response"))?;

        info!("LLM output: {raw}");

        let mut output: LlmWeekOutput = serde_json::from_str(raw)?;

        // Validate lunch_start/lunch_end coherence (schema only validates enum, not pairing)
        output.fix_pairs();
        output.validate(expected_days).map_err(|e| anyhow::anyhow!(e))?;

        Ok(output)
    }
}
