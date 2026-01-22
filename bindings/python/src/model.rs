//! Python wrapper for `SystemLanguageModel` and `ModelAvailability`.

use std::sync::Arc;

use pyo3::prelude::*;

use crate::error::to_py_err;

/// Represents the availability status of a `FoundationModel`.
#[pyclass(eq, eq_int, module = "fm")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelAvailability {
    /// Model is available and ready to use.
    Available,
    /// Device is not eligible for Apple Intelligence.
    DeviceNotEligible,
    /// Apple Intelligence is not enabled in system settings.
    AppleIntelligenceNotEnabled,
    /// Model is not ready (downloading or other system reasons).
    ModelNotReady,
    /// Unavailability for an unknown reason.
    Unknown,
}

impl From<fm_rs::ModelAvailability> for ModelAvailability {
    fn from(a: fm_rs::ModelAvailability) -> Self {
        match a {
            fm_rs::ModelAvailability::Available => ModelAvailability::Available,
            fm_rs::ModelAvailability::DeviceNotEligible => ModelAvailability::DeviceNotEligible,
            fm_rs::ModelAvailability::AppleIntelligenceNotEnabled => {
                ModelAvailability::AppleIntelligenceNotEnabled
            }
            fm_rs::ModelAvailability::ModelNotReady => ModelAvailability::ModelNotReady,
            fm_rs::ModelAvailability::Unknown => ModelAvailability::Unknown,
        }
    }
}

/// The system language model provided by Apple Intelligence.
///
/// This is the main entry point for using on-device AI capabilities.
///
/// Example:
///     model = `SystemLanguageModel()`
///     if `model.is_available`:
///         print("Model is ready to use!")
#[pyclass(module = "fm")]
#[derive(Clone)]
pub struct SystemLanguageModel {
    inner: Arc<fm_rs::SystemLanguageModel>,
}

#[pymethods]
impl SystemLanguageModel {
    /// Creates the default system language model.
    ///
    /// Raises:
    ///     `ModelNotAvailableError`: If the model cannot be created.
    #[new]
    fn new() -> PyResult<Self> {
        let model = fm_rs::SystemLanguageModel::new().map_err(to_py_err)?;
        Ok(Self {
            inner: Arc::new(model),
        })
    }

    /// Returns True if the model is available and ready to use.
    #[getter]
    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    /// Gets the current availability status of the model.
    ///
    /// Returns:
    ///     `ModelAvailability`: The availability status.
    #[getter]
    fn availability(&self) -> ModelAvailability {
        self.inner.availability().into()
    }

    /// Returns an error if the model is unavailable.
    ///
    /// Raises:
    ///     `DeviceNotEligibleError`: If device is not eligible.
    ///     `AppleIntelligenceNotEnabledError`: If Apple Intelligence is not enabled.
    ///     `ModelNotReadyError`: If model is not ready.
    ///     `ModelNotAvailableError`: If unavailable for unknown reason.
    fn ensure_available(&self) -> PyResult<()> {
        self.inner.ensure_available().map_err(to_py_err)
    }

    fn __repr__(&self) -> String {
        let avail = match self.inner.availability() {
            fm_rs::ModelAvailability::Available => "Available",
            fm_rs::ModelAvailability::DeviceNotEligible => "DeviceNotEligible",
            fm_rs::ModelAvailability::AppleIntelligenceNotEnabled => "AppleIntelligenceNotEnabled",
            fm_rs::ModelAvailability::ModelNotReady => "ModelNotReady",
            fm_rs::ModelAvailability::Unknown => "Unknown",
        };
        format!("SystemLanguageModel(availability={avail})")
    }
}

impl SystemLanguageModel {
    /// Returns a reference to the inner fm-rs model wrapped in Arc.
    pub fn inner(&self) -> &Arc<fm_rs::SystemLanguageModel> {
        &self.inner
    }
}
