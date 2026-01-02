use crate::auth::XblToken;
use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SocialSummary {
    #[serde(default)]
    pub people: Vec<Person>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Person {
    pub xuid: String,
    #[serde(rename = "gamertag")]
    pub gamertag: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "isFollowedByCaller")]
    pub is_followed_by_caller: bool,
    #[serde(rename = "isFollowingCaller")]
    pub is_following_caller: bool,
}

pub struct FriendsClient {
    client: reqwest::Client,
}

impl FriendsClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn get_summary(&self, token: &XblToken, url: &str) -> XblResult<SocialSummary> {
        let response = self
            .client
            .get(url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "5")
            .header("accept-language", "en-GB")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Failed to get friends: {}",
                response.status()
            )));
        }

        let text = response.text().await?;
        if text.is_empty() {
            return Ok(SocialSummary { people: vec![] });
        }

        let summary: SocialSummary = serde_json::from_str(&text)?;
        Ok(summary)
    }

    pub async fn add_friend(&self, token: &XblToken, xuid: &str) -> XblResult<()> {
        let url = format!("https://social.xboxlive.com/users/me/people/xuid({})", xuid);
        self.client
            .put(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;
        Ok(())
    }

    pub async fn remove_friend(&self, token: &XblToken, xuid: &str) -> XblResult<()> {
        let url = format!("https://social.xboxlive.com/users/me/people/xuid({})", xuid);
        self.client
            .delete(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;
        Ok(())
    }

    pub async fn get_incoming_requests(&self, token: &XblToken) -> XblResult<Vec<String>> {
        let url = "https://peoplehub.xboxlive.com/users/me/people/friendrequests(received)";
        let summary = self.get_summary(token, url).await?;
        Ok(summary.people.into_iter().map(|p| p.xuid).collect())
    }

    pub async fn accept_requests(&self, token: &XblToken, xuids: Vec<String>) -> XblResult<()> {
        if xuids.is_empty() {
            return Ok(());
        }

        let url = "https://social.xboxlive.com/bulk/users/me/people/friends/v2?method=add";
        let body = serde_json::json!({ "xuids": xuids });

        self.client
            .post(url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "2")
            .json(&body)
            .send()
            .await?;
        Ok(())
    }
}
