use thiserror::Error;

/// Possible errors when using the mint lib.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MintError {
    /// Failed to update.
    #[error("failed to fetch github release: {summary}")]
    FetchGithubReleaseFailed {
        summary: String,
        details: Option<String>,
    },

    /// Failed to locate Deep Rock Galactic installation.
    #[error("unable to locate Deep Rock Galactic installation: {summary}")]
    UnknownInstallation {
        summary: String,
        details: Option<String>,
    },

    /// Failed to setup tracing.
    #[error("failed to setup logging")]
    LogSetupFailed {
        summary: String,
        details: Option<String>,
    },
}
