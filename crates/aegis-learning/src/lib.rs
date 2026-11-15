//! AEGIS LEARNING — feature engineering, model inference, validation gate and
//! live monitoring. Training lives in `research/` (Python); this crate is what
//! the engine runs. It is statistical machine learning, not a chatbot.

// `!(a >= b)` is used on purpose: unlike `a < b`, it is also true when a value is NaN,
// so a missing or corrupt number can never pass a check.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod features;
pub mod model;
pub mod monitor;

pub use features::{build_dataset, compute as compute_features, dataset_to_csv, FEATURE_NAMES};
pub use model::{GateCriteria, GateResult, ModelArtifact, ModelError, ValidationReport};
pub use monitor::{DriftReport, LivePerformance, ModelMonitor};
