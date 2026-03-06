use crate::auth::{XblToken, sign_request_for_url, update_server_time_from_header};
use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};

/// Maximum number of friends allowed by Xbox Live.
/// Increased from 1000 to 2000 in August 2024.
pub const MAX_FRIENDS: usize = 2000;

#[derive(Debug, Serialize, Deserialize)]
pub struct SocialSummary {
    #[serde(default)]
    pub people: Vec<Person>,
}

/// Summary of social relationship counts from Xbox Live.
#[derive(Debug, Clone)]
pub struct SocialSummaryInfo {
    /// Number of people we are following.
    pub following_count: i64,
    /// Number of people following us.
    pub follower_count: i64,
    /// Remaining friend slots before hitting the limit.
    pub available_slots: i64,
}

/// Raw social summary response from Xbox.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SocialSummaryResponse {
    #[serde(default)]
    target_following_count: i64,
    #[serde(default)]
    target_follower_count: i64,
    #[serde(default)]
    available_people_slots: i64,
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

    fn signature(method: &str, url: &str, authorization: &str, body: &[u8]) -> XblResult<String> {
        sign_request_for_url(method, url, authorization, body)
            .map_err(|e| XblError::Auth(format!("Failed to sign Xbox request: {}", e)))
    }

    fn sync_server_time(response: &reqwest::Response) {
        if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
            update_server_time_from_header(date);
        }
    }

    pub async fn get_summary(&self, token: &XblToken, url: &str) -> XblResult<SocialSummary> {
        let authorization = token.auth_header();
        let signature = Self::signature("GET", url, &authorization, b"")?;

        let response = self
            .client
            .get(url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "5")
            .header("accept-language", "en-GB")
            .send()
            .await?;
        Self::sync_server_time(&response);

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
        let authorization = token.auth_header();
        let signature = Self::signature("PUT", &url, &authorization, b"")?;

        let response = self
            .client
            .put(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "2")
            .header("Content-Length", "0")
            .send()
            .await?;
        Self::sync_server_time(&response);

        // 204 = success, friend added
        // 400 with code 1028 = friend list full
        // 429 = rate limited
        let status = response.status();
        if status.as_u16() == 204 {
            Ok(())
        } else if status.as_u16() == 429 {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let message = match retry_after {
                Some(secs) => {
                    format!("Rate limited - too many friend requests (retry after {secs}s)")
                }
                None => "Rate limited - too many friend requests".into(),
            };
            Err(XblError::XboxLive(message))
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
        let authorization = token.auth_header();
        let signature = Self::signature("DELETE", &url, &authorization, b"")?;

        let response = self
            .client
            .delete(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;
        Self::sync_server_time(&response);
        Ok(())
    }

    pub async fn get_incoming_requests(&self, token: &XblToken) -> XblResult<Vec<String>> {
        let url = "https://peoplehub.xboxlive.com/users/me/people/friendrequests(received)";
        let authorization = token.auth_header();
        let signature = Self::signature("GET", url, &authorization, b"")?;

        let response = self
            .client
            .get(url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "7")
            .header("accept-language", "en-GB")
            .send()
            .await?;
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Failed to get friend requests: {}",
                response.status()
            )));
        }

        let text = response.text().await?;
        if text.is_empty() {
            return Ok(vec![]);
        }

        let summary: SocialSummary = serde_json::from_str(&text)?;
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

    /// Get list of people we follow (our social/following list).
    ///
    /// This returns everyone we are following, regardless of whether they follow us back.
    pub async fn get_social(&self, token: &XblToken) -> XblResult<Vec<Person>> {
        let url = "https://peoplehub.xboxlive.com/users/me/people/social";
        let summary = self.get_summary(token, url).await?;
        Ok(summary.people)
    }

    /// Get a merged view of followers and social (people we follow).
    ///
    /// This combines both lists by XUID, OR-ing the boolean flags
    /// (`is_following_caller`, `is_followed_by_caller`). This matches
    /// Broadcaster's `FriendManager.get()` merge behavior and gives a
    /// complete picture of the relationship state for each person.
    pub async fn get_merged_friends(&self, token: &XblToken) -> XblResult<Vec<Person>> {
        let followers = self.get_followers(token).await?;
        let social = self.get_social(token).await?;

        let mut merged: std::collections::HashMap<String, Person> =
            std::collections::HashMap::new();

        for person in followers {
            merged
                .entry(person.xuid.clone())
                .and_modify(|existing| {
                    existing.is_following_caller =
                        existing.is_following_caller || person.is_following_caller;
                    existing.is_followed_by_caller =
                        existing.is_followed_by_caller || person.is_followed_by_caller;
                })
                .or_insert(person);
        }

        for person in social {
            merged
                .entry(person.xuid.clone())
                .and_modify(|existing| {
                    existing.is_following_caller =
                        existing.is_following_caller || person.is_following_caller;
                    existing.is_followed_by_caller =
                        existing.is_followed_by_caller || person.is_followed_by_caller;
                })
                .or_insert(person);
        }

        Ok(merged.into_values().collect())
    }

    /// Get social summary with friend counts and available slots.
    ///
    /// Uses the `social.xboxlive.com/users/me/summary` endpoint to get
    /// following/follower counts and remaining friend slots.
    pub async fn get_social_summary(&self, token: &XblToken) -> XblResult<SocialSummaryInfo> {
        let url = "https://social.xboxlive.com/users/me/summary";
        let authorization = token.auth_header();
        let signature = Self::signature("GET", url, &authorization, b"")?;

        let response = self
            .client
            .get(url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "5")
            .send()
            .await?;
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Failed to get social summary: {}",
                response.status()
            )));
        }

        let resp: SocialSummaryResponse = response.json().await?;
        Ok(SocialSummaryInfo {
            following_count: resp.target_following_count,
            follower_count: resp.target_follower_count,
            available_slots: resp.available_people_slots,
        })
    }

    /// Bulk add friends (accept requests or follow back).
    ///
    /// Returns the list of XUIDs that were successfully added.
    /// Automatically chunks requests and falls back to single-add mode if
    /// the bulk endpoint fails (which happens intermittently on some accounts).
    pub async fn add_friends_bulk(
        &self,
        token: &XblToken,
        xuids: &[String],
    ) -> XblResult<Vec<String>> {
        if xuids.is_empty() {
            return Ok(vec![]);
        }

        // Xbox limits bulk operations to ~100 XUIDs per request
        const CHUNK_SIZE: usize = 50;

        let mut all_added = Vec::new();

        for chunk in xuids.chunks(CHUNK_SIZE) {
            match self.add_friends_bulk_single(token, chunk).await {
                Ok(added) => all_added.extend(added),
                Err(e) => {
                    // If rate limited, return what we have so far
                    if e.to_string().contains("Rate limited") {
                        if all_added.is_empty() {
                            return Err(e);
                        }
                        // Return partial results
                        return Ok(all_added);
                    }

                    // Bulk endpoints can fail while single-add still works.
                    let added = match self.add_friends_individual(token, chunk).await {
                        Ok(added) => added,
                        Err(e) => {
                            if e.to_string().contains("Rate limited") {
                                if all_added.is_empty() {
                                    return Err(e);
                                }
                                return Ok(all_added);
                            }
                            return Err(e);
                        }
                    };

                    all_added.extend(added);
                }
            }
        }

        Ok(all_added)
    }

    /// Internal: Add a single batch of friends (up to CHUNK_SIZE).
    async fn add_friends_bulk_single(
        &self,
        token: &XblToken,
        xuids: &[String],
    ) -> XblResult<Vec<String>> {
        let url = "https://social.xboxlive.com/bulk/users/me/people/friends/v2?method=add";
        let body = serde_json::json!({ "xuids": xuids });
        let body_bytes = serde_json::to_vec(&body)?;
        let authorization = token.auth_header();
        let signature = Self::signature("POST", url, &authorization, &body_bytes)?;

        let response = self
            .client
            .post(url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "2")
            .body(body_bytes)
            .send()
            .await?;
        Self::sync_server_time(&response);

        let status = response.status();
        if status.as_u16() == 429 {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let message = match retry_after {
                Some(secs) => {
                    format!("Rate limited - too many friend requests (retry after {secs}s)")
                }
                None => "Rate limited - too many friend requests".into(),
            };
            return Err(XblError::XboxLive(message));
        }

        // Parse response to get which XUIDs were actually added
        // Response format: { "updatedPeople": ["xuid1", "xuid2", ...] }
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(XblError::XboxLive(format!(
                "Bulk add friends failed: {} - {}",
                status, text
            )));
        }

        // Try to parse the response
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(updated) = json.get("updatedPeople").and_then(|v| v.as_array())
        {
            let added: Vec<String> = updated
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            return Ok(added);
        }

        // If we can't parse, assume all were added (old behavior)
        Ok(xuids.to_vec())
    }

    /// Fallback path: add friends individually.
    ///
    /// This mirrors the behavior used by older broadcaster implementations and
    /// tends to be more stable when bulk mutations are rejected.
    async fn add_friends_individual(
        &self,
        token: &XblToken,
        xuids: &[String],
    ) -> XblResult<Vec<String>> {
        let mut added = Vec::new();

        for xuid in xuids {
            match self.add_friend(token, xuid).await {
                Ok(_) => added.push(xuid.clone()),
                Err(e) => {
                    let err = e.to_string();

                    if err.contains("Rate limited") {
                        if added.is_empty() {
                            return Err(e);
                        }
                        return Ok(added);
                    }

                    if Self::is_friend_list_full_error(&err) {
                        return Err(XblError::XboxLive(
                            "Friend list full - cannot add more friends".into(),
                        ));
                    }

                    // Account/privacy restrictions (codes 1011, 1049) — force-remove
                    // them as a follower so they stop appearing in every sync cycle.
                    // Matches Broadcaster's forceUnfollow() behavior.
                    if Self::is_friend_restriction_error(&err) {
                        if let Err(e) = self.force_remove_follower(token, xuid).await {
                            tracing::debug!(
                                xuid,
                                "Failed to force-remove restricted follower: {}",
                                e
                            );
                        } else {
                            tracing::info!(
                                xuid,
                                "Force-removed restricted follower (privacy settings block friend requests)"
                            );
                        }
                        continue;
                    }

                    return Err(e);
                }
            }
        }

        Ok(added)
    }

    fn is_friend_list_full_error(err: &str) -> bool {
        err.contains("\"code\":1028")
            || err.contains("\"code\": 1028")
            || err.contains("code 1028")
            || err.contains("Code\":1028")
    }

    fn is_friend_restriction_error(err: &str) -> bool {
        err.contains("\"code\":1011")
            || err.contains("\"code\": 1011")
            || err.contains("code 1011")
            || err.contains("Code\":1011")
            || err.contains("\"code\":1049")
            || err.contains("\"code\": 1049")
            || err.contains("code 1049")
            || err.contains("Code\":1049")
    }

    pub async fn accept_requests(&self, token: &XblToken, xuids: Vec<String>) -> XblResult<()> {
        self.add_friends_bulk(token, &xuids).await?;
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
        let authorization = token.auth_header();
        let signature = Self::signature("DELETE", &url, &authorization, b"")?;
        let response = self
            .client
            .delete(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;
        Self::sync_server_time(&response);

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
        let authorization = token.auth_header();
        let signature = Self::signature("GET", &url, &authorization, b"")?;

        let response = self
            .client
            .get(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "2")
            .send()
            .await?;
        Self::sync_server_time(&response);

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
