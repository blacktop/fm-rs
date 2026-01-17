//! Generation options for controlling model output.

use serde::{Deserialize, Serialize};

/// Sampling strategy for token generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sampling {
    /// Greedy sampling: always pick the most likely token.
    Greedy,
    /// Random sampling with temperature.
    #[default]
    Random,
}

/// Options that control how the model generates its response.
///
/// Use the builder pattern to configure options:
///
/// ```rust
/// use fm_rs::GenerationOptions;
///
/// let options = GenerationOptions::builder()
///     .temperature(0.7)
///     .max_response_tokens(500)
///     .build();
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationOptions {
    /// Temperature for sampling (0.0-2.0).
    /// Higher values produce more random outputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Sampling strategy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Sampling>,

    /// Maximum number of tokens in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "maximumResponseTokens")]
    pub max_response_tokens: Option<u32>,

    /// Random seed for reproducible generation.
    ///
    /// **Note**: This field is currently not supported by Apple's `GenerationOptions` API
    /// and is ignored. It is included for potential future use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl GenerationOptions {
    /// Creates a new builder for configuring generation options.
    pub fn builder() -> GenerationOptionsBuilder {
        GenerationOptionsBuilder::default()
    }

    /// Serializes the options to JSON for FFI.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Builder for configuring [`GenerationOptions`].
#[derive(Debug, Default)]
pub struct GenerationOptionsBuilder {
    temperature: Option<f64>,
    sampling: Option<Sampling>,
    max_response_tokens: Option<u32>,
    seed: Option<u64>,
}

impl GenerationOptionsBuilder {
    /// Sets the temperature for generation.
    ///
    /// Temperature influences the confidence of the model's response.
    /// Higher values (e.g., 1.5) produce more random outputs.
    /// Lower values (e.g., 0.2) produce more deterministic outputs.
    ///
    /// Valid range: 0.0 to 2.0. Values outside this range are ignored
    /// and the default temperature is used instead.
    pub fn temperature(mut self, temp: f64) -> Self {
        if (0.0..=2.0).contains(&temp) {
            self.temperature = Some(temp);
        }
        self
    }

    /// Sets the temperature, returning an error if out of range.
    ///
    /// This is the fallible version of [`temperature`](Self::temperature).
    /// Use this when you want to catch invalid temperature values at build time.
    ///
    /// # Errors
    ///
    /// Returns an error if `temp` is not in the range 0.0 to 2.0.
    pub fn try_temperature(mut self, temp: f64) -> Result<Self, crate::Error> {
        if (0.0..=2.0).contains(&temp) {
            self.temperature = Some(temp);
            Ok(self)
        } else {
            Err(crate::Error::InvalidInput(format!(
                "Temperature must be between 0.0 and 2.0, got {temp}"
            )))
        }
    }

    /// Sets the sampling strategy.
    pub fn sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = Some(sampling);
        self
    }

    /// Sets the maximum number of tokens in the response.
    ///
    /// Only use this when you need to protect against unexpectedly verbose responses.
    /// Enforcing a strict token limit can lead to malformed or grammatically incorrect output.
    pub fn max_response_tokens(mut self, tokens: u32) -> Self {
        if tokens > 0 {
            self.max_response_tokens = Some(tokens);
        }
        self
    }

    /// Sets the random seed for reproducible generation.
    ///
    /// **Note**: This is currently not supported by Apple's `GenerationOptions` API
    /// and will be ignored. Included for potential future use.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Builds the [`GenerationOptions`].
    pub fn build(self) -> GenerationOptions {
        GenerationOptions {
            temperature: self.temperature,
            sampling: self.sampling,
            max_response_tokens: self.max_response_tokens,
            seed: self.seed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let options = GenerationOptions::default();
        assert!(options.temperature.is_none());
        assert!(options.sampling.is_none());
        assert!(options.max_response_tokens.is_none());
    }

    #[test]
    fn test_builder() {
        let options = GenerationOptions::builder()
            .temperature(0.7)
            .sampling(Sampling::Random)
            .max_response_tokens(500)
            .seed(42)
            .build();

        assert_eq!(options.temperature, Some(0.7));
        assert_eq!(options.sampling, Some(Sampling::Random));
        assert_eq!(options.max_response_tokens, Some(500));
        assert_eq!(options.seed, Some(42));
    }

    #[test]
    fn test_temperature_bounds() {
        // Valid temperature
        let options = GenerationOptions::builder().temperature(1.5).build();
        assert_eq!(options.temperature, Some(1.5));

        // Out of bounds (negative)
        let options = GenerationOptions::builder().temperature(-0.5).build();
        assert!(options.temperature.is_none());

        // Out of bounds (too high)
        let options = GenerationOptions::builder().temperature(3.0).build();
        assert!(options.temperature.is_none());
    }

    #[test]
    fn test_json_serialization() {
        let options = GenerationOptions::builder()
            .temperature(0.7)
            .max_response_tokens(100)
            .build();

        let json = options.to_json();
        assert!(json.contains("temperature"));
        assert!(json.contains("0.7"));
    }
}
