use thiserror::Error;

use crate::providers::ModSpecification;

#[derive(Error, Debug)]
pub enum IntegrationError {
    #[error("No provider found for {:?}", spec)]
    NoProvider {
        spec: ModSpecification,
        factory: &'static crate::providers::ProviderFactory,
    },
}
