//! Python wrappers for `GenerationOptions` and Sampling.

use pyo3::prelude::*;

/// Sampling strategy for token generation.
#[pyclass(eq, eq_int, module = "fm")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sampling {
    /// Greedy sampling: always pick the most likely token.
    Greedy,
    /// Random sampling with temperature.
    Random,
}

impl From<Sampling> for fm_rs::Sampling {
    fn from(s: Sampling) -> Self {
        match s {
            Sampling::Greedy => fm_rs::Sampling::Greedy,
            Sampling::Random => fm_rs::Sampling::Random,
        }
    }
}

impl From<fm_rs::Sampling> for Sampling {
    fn from(s: fm_rs::Sampling) -> Self {
        match s {
            fm_rs::Sampling::Greedy => Sampling::Greedy,
            fm_rs::Sampling::Random => Sampling::Random,
        }
    }
}

/// Options that control how the model generates its response.
///
/// Example:
///     options = GenerationOptions(temperature=0.7, `max_response_tokens=500`)
#[pyclass(module = "fm")]
#[derive(Debug, Clone, Default)]
pub struct GenerationOptions {
    inner: fm_rs::GenerationOptions,
}

#[pymethods]
impl GenerationOptions {
    /// Creates a new `GenerationOptions` instance.
    ///
    /// Args:
    ///     temperature: Temperature for sampling (0.0-2.0). Higher values produce more random outputs.
    ///     sampling: Sampling strategy (Greedy or Random).
    ///     `max_response_tokens`: Maximum number of tokens in the response.
    ///     seed: Random seed for reproducible generation (currently not supported by Apple's API).
    #[new]
    #[pyo3(signature = (*, temperature=None, sampling=None, max_response_tokens=None, seed=None))]
    fn new(
        temperature: Option<f64>,
        sampling: Option<Sampling>,
        max_response_tokens: Option<u32>,
        seed: Option<u64>,
    ) -> PyResult<Self> {
        // Validate temperature if provided
        if let Some(temp) = temperature {
            if !(0.0..=2.0).contains(&temp) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Temperature must be between 0.0 and 2.0",
                ));
            }
        }

        let inner = fm_rs::GenerationOptions {
            temperature,
            sampling: sampling.map(Into::into),
            max_response_tokens,
            seed,
        };

        Ok(Self { inner })
    }

    /// Temperature for sampling (0.0-2.0).
    #[getter]
    fn temperature(&self) -> Option<f64> {
        self.inner.temperature
    }

    /// Sampling strategy.
    #[getter]
    fn sampling(&self) -> Option<Sampling> {
        self.inner.sampling.map(Into::into)
    }

    /// Maximum number of tokens in the response.
    #[getter]
    fn max_response_tokens(&self) -> Option<u32> {
        self.inner.max_response_tokens
    }

    /// Random seed (currently not supported).
    #[getter]
    fn seed(&self) -> Option<u64> {
        self.inner.seed
    }

    fn __repr__(&self) -> String {
        let mut parts = Vec::new();
        if let Some(t) = self.inner.temperature {
            parts.push(format!("temperature={t}"));
        }
        if let Some(s) = self.inner.sampling {
            let s_str = match s {
                fm_rs::Sampling::Greedy => "Sampling.Greedy",
                fm_rs::Sampling::Random => "Sampling.Random",
            };
            parts.push(format!("sampling={s_str}"));
        }
        if let Some(m) = self.inner.max_response_tokens {
            parts.push(format!("max_response_tokens={m}"));
        }
        if let Some(s) = self.inner.seed {
            parts.push(format!("seed={s}"));
        }
        format!("GenerationOptions({})", parts.join(", "))
    }
}

impl GenerationOptions {
    /// Returns a reference to the inner fm-rs `GenerationOptions`.
    pub fn inner(&self) -> &fm_rs::GenerationOptions {
        &self.inner
    }
}
