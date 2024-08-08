use crate::error::MintError;

pub const GITHUB_RELEASE_URL: &str = "https://api.github.com/repos/trumank/mint/releases/latest";
pub const GITHUB_REQ_USER_AGENT: &str = "trumank/mint";

#[derive(Debug, serde::Deserialize)]
pub struct GitHubRelease {
    pub html_url: String,
    pub tag_name: String,
    pub body: String,
}

fn fail_reqwest<S: AsRef<str>>(e: reqwest::Error, error_summary: S) -> MintError {
    MintError::FetchGithubReleaseFailed {
        summary: error_summary.as_ref().to_string(),
        details: Some(e.to_string()),
    }
}

pub async fn get_latest_release() -> Result<GitHubRelease, MintError> {
    let client = reqwest::Client::builder()
        .user_agent(GITHUB_REQ_USER_AGENT)
        .build()
        .map_err(|e| fail_reqwest(e, "failed to construct reqwest client"))?;

    let response =
        client.get(GITHUB_RELEASE_URL).send().await.map_err(|e| {
            fail_reqwest(e, "failed to receive response from `{GITHUB_RELEASE_URL}`")
        })?;

    let rel_info = response
        .json::<GitHubRelease>()
        .await
        .map_err(|e| fail_reqwest(e, "failed to deserialize github release info"))?;

    Ok(rel_info)
}
