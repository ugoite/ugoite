//! Head-owned, exact active Pin values.

use crate::publication_ref::PublicationRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PinEntry {
    pub coordinate: PublicationRef,
    pub created_at_micros: i64,
    pub created_by_principal_id: String,
}

impl PinEntry {
    pub fn validate(&self) -> Result<(), PinValidationError> {
        self.coordinate
            .validate()
            .map_err(|_| PinValidationError::InvalidCoordinate)?;
        if self.created_by_principal_id.trim().is_empty() {
            return Err(PinValidationError::MissingCreator);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PinValidationError {
    InvalidCoordinate,
    MissingCreator,
}

impl std::fmt::Display for PinValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCoordinate => "pin coordinate is invalid",
            Self::MissingCreator => "pin creator must not be empty",
        })
    }
}

impl std::error::Error for PinValidationError {}
