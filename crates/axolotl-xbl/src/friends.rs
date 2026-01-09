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
        let response = self
            .client
            .put(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;

        // 204 = success, friend added
        // 400 with code 1028 = friend list full
        // 429 = rate limited
        let status = response.status();
        if status.as_u16() == 204 {
            Ok(())
        } else if status.as_u16() == 429 {
            Err(XblError::XboxLive("Rate limited - too many friend requests".into()))
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(XblError::XboxLive(format!(
                "Failed to add friend: {} - {}",
                status, body
            )))
        }
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

    /// Get list of followers (people following us) with full person info.
    ///
    /// This is used for periodic friend sync to auto-follow back.
    pub async fn get_followers(&self, token: &XblToken) -> XblResult<Vec<Person>> {
        let url = "https://peoplehub.xboxlive.com/users/me/people/followers";
        let summary = self.get_summary(token, url).await?;
        Ok(summary.people)
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

    /// Force remove a follower (someone following you).
    ///
    /// This is different from `remove_friend` which removes someone you're following.
    /// Use this to block attackers who may have friended you to access your sessions.
    pub async fn force_remove_follower(&self, token: &XblToken, xuid: &str) -> XblResult<()> {
        // First unfriend them (if we follow them)
        self.remove_friend(token, xuid).await.ok();

        // Then force remove them as a follower
        let url = format!(
            "https://social.xboxlive.com/users/me/people/follower/xuid({})",
            xuid
        );
        let response = self
            .client
            .delete(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Failed to remove follower: {}",
                response.status()
            )));
        }
        Ok(())
    }

    /// Get gamertag for an XUID.
    pub async fn get_gamertag(&self, token: &XblToken, xuid: &str) -> XblResult<String> {
        let url = format!(
            "https://profile.xboxlive.com/users/xuid({})/profile/settings?settings=Gamertag",
            xuid
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Failed to get profile: {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response.json().await?;

        // Parse gamertag from response
        let gamertag = json
            .get("profileUsers")
            .and_then(|u| u.as_array())
            .and_then(|a| a.first())
            .and_then(|u| u.get("settings"))
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|s| s.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        Ok(gamertag)
    }
}

impl Default for FriendsClient {
    fn default() -> Self {
        Self::new()
    }
}
