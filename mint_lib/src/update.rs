use crate::error::GenericError;
use crate::error::ResultExt;

pub const GITHUB_RELEASE_URL: &str = "https://api.github.com/repos/bluecookiefrog/mint/releases/latest";
pub const GITHUB_REQ_USER_AGENT: &str = "bluecookiefrog/mint";

#[derive(Debug, serde::Deserialize)]
pub struct GitHubRelease {
    pub html_url: String,
    pub tag_name: String,
    pub body: String,
}

pub async fn get_latest_release() -> Result<GitHubRelease, GenericError> {
    reqwest::Client::builder()
        .user_agent(GITHUB_REQ_USER_AGENT)
        .build()
        .generic("failed to construct reqwest client".to_string())?
        .get(GITHUB_RELEASE_URL)
        .send()
        .await
        .generic("check self update request failed".to_string())?
        .json::<GitHubRelease>()
        .await
        .generic("check self update response is error".to_string())
}
