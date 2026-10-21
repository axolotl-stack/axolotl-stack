//! Social wire references: df-mc/go-xsapi social/relationship.go:Follow,
//! RemoveMutualFollow, RemoveFollower, AddFriends and social/user.go:users.
//! Undecorated version-5 polling is the upstream PeopleListConfig override.
//! Social-summary counts and the 2000-slot policy are retained application
//! extensions, not constants provided by go-xsapi.
use crate::auth::{XblToken, sign_service_request, update_server_time_from_header};
use crate::error::{XblError, XblResult, response_json};
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
    #[serde(rename = "gamertag", default)]
    pub gamertag: String,
    #[serde(rename = "displayName", default)]
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

    // df-mc/go-xsapi social/relationship.go:doRelationship and social/error.go:responseError.
    // Only numeric metadata is retained: descriptions and source fields may echo credentials.
    async fn relationship_result(
        response: reqwest::Response,
        operation: &'static str,
        success_codes: &[u16],
    ) -> XblResult<()> {
        let status = response.status();
        if success_codes.contains(&status.as_u16()) {
            return Ok(());
        }
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        #[derive(Deserialize)]
        struct ErrorCode {
            #[serde(alias = "Code")]
            code: Option<i64>,
        }
        let code = match response.bytes().await {
            Ok(bytes) => serde_json::from_slice::<ErrorCode>(&bytes)
                .ok()
                .and_then(|v| v.code),
            Err(_) => None,
        };
        if code.is_some()
            || retry_after.is_some()
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            Err(XblError::SocialStatus {
                status,
                code,
                retry_after,
            })
        } else {
            Err(XblError::HttpStatus { status, operation })
        }
    }

    fn sync_server_time(response: &reqwest::Response) {
        if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
            update_server_time_from_header(date);
        }
    }

    pub async fn get_summary(&self, token: &XblToken, url: &str) -> XblResult<SocialSummary> {
        let mut request = self
            .client
            .get(url)
            .header("x-xbl-contract-version", "5")
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);

        response_json(response, "get friends").await
    }

    pub async fn add_friend(&self, token: &XblToken, xuid: &str) -> XblResult<()> {
        let url = format!("https://social.xboxlive.com/users/me/people/xuid({})", xuid);
        let mut request = self
            .client
            .put(&url)
            .header("x-xbl-contract-version", "3")
            .header("Accept", "application/json")
            .header("Cache-Control", "no-cache")
            .header("Accept-Language", "en-US,en")
            .header("Content-Length", "0")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);

        // social/relationship.go:Follow (legacy one-way follow, not AddFriend).
        Self::relationship_result(response, "add friend", &[200, 204]).await
    }

    pub async fn remove_friend(&self, token: &XblToken, xuid: &str) -> XblResult<()> {
        let url = format!("https://social.xboxlive.com/users/me/people/xuid({})", xuid);
        let mut request = self
            .client
            .delete(&url)
            .header("x-xbl-contract-version", "3")
            .header("Accept", "application/json")
            .header("Cache-Control", "no-cache")
            .header("Accept-Language", "en-US,en")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);
        // social/relationship.go:RemoveMutualFollow.
        Self::relationship_result(response, "remove friend", &[200, 204]).await
    }

    pub async fn get_incoming_requests(&self, token: &XblToken) -> XblResult<Vec<String>> {
        let url = "https://peoplehub.xboxlive.com/users/me/people/friendRequests(received)/decoration/bio,detail,multiplayerSummary,preferredColor,presenceDetail";
        let mut request = self
            .client
            .get(url)
            .header("x-xbl-contract-version", "7")
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);

        let summary: SocialSummary = response_json(response, "get friend requests").await?;
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
        let mut request = self
            .client
            .get(url)
            .header("x-xbl-contract-version", "5")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);

        let resp: SocialSummaryResponse = response_json(response, "get social summary").await?;
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
                    if e.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
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
                            if e.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
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
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "3")
            .header("Accept", "application/json")
            .header("Cache-Control", "no-cache")
            .header("Accept-Language", "en-US,en")
            .body(body_bytes)
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);

        // social/relationship.go:AddFriends/doBulkFriends: never invent success
        // for malformed or missing updatedPeople.
        if !matches!(response.status().as_u16(), 200 | 201) {
            return Self::relationship_result(response, "bulk add friends", &[200, 201])
                .await
                .map(|()| Vec::new());
        }
        #[derive(Deserialize)]
        struct BulkResult {
            #[serde(rename = "updatedPeople", default)]
            updated: Vec<String>,
        }
        let result: BulkResult = response_json(response, "bulk add friends").await?;
        Ok(result.updated)
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
                    if e.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
                        if added.is_empty() {
                            return Err(e);
                        }
                        return Ok(added);
                    }

                    if e.social_code() == Some(1028) {
                        return Err(e);
                    }

                    // Account/privacy restrictions (codes 1011, 1049) — force-remove
                    // them as a follower so they stop appearing in every sync cycle.
                    // Matches Broadcaster's forceUnfollow() behavior.
                    if matches!(e.social_code(), Some(1011 | 1049)) {
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
        let mut request = self
            .client
            .delete(&url)
            .header("x-xbl-contract-version", "3")
            .header("Accept", "application/json")
            .header("Cache-Control", "no-cache")
            .header("Accept-Language", "en-US,en")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        Self::sync_server_time(&response);

        Self::relationship_result(response, "remove follower", &[200, 204]).await
    }

    /// Get gamertag for an XUID (social/user.go:UserByXUID).
    pub async fn get_gamertag(&self, token: &XblToken, xuid: &str) -> XblResult<String> {
        let url = format!(
            "https://peoplehub.xboxlive.com/users/me/people/xuids({})/decoration/bio,detail,multiplayerSummary,preferredColor,presenceDetail",
            xuid
        );
        let mut request = self
            .client
            .get(&url)
            .header("x-xbl-contract-version", "7")
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en")
            .build()?;
        sign_service_request(&mut request, token).await?;
        let response = self.client.execute(request).await?;
        #[derive(Deserialize)]
        struct Profile {
            xuid: String,
            gamertag: String,
        }
        #[derive(Deserialize)]
        struct Profiles {
            people: Vec<Profile>,
        }
        let summary: Profiles = response_json(response, "get profile").await?;
        let gamertag = summary
            .people
            .into_iter()
            .find(|profile| profile.xuid == xuid)
            .ok_or_else(|| XblError::XboxLive("No Xbox profile found".into()))?
            .gamertag;

        Ok(gamertag)
    }
}

impl Default for FriendsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    async fn response(status: u16, body: &str) -> reqwest::Response {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let reply = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert_ne!(stream.read(&mut request).unwrap(), 0);
            stream.write_all(reply.as_bytes()).unwrap();
        });
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        server.join().unwrap();
        response
    }

    #[tokio::test]
    async fn social_errors_keep_codes_without_echoed_secrets() {
        for (status, body, code) in [
            (
                400,
                r#"{"code":1028,"description":"ECHOED_SECRET"}"#,
                Some(1028),
            ),
            (403, r#"{"code":1011,"source":"ECHOED_SECRET"}"#, Some(1011)),
            (429, "ECHOED_SECRET", None),
            (500, r#"{"code":"ECHOED_SECRET"}"#, None),
        ] {
            let error = FriendsClient::relationship_result(
                response(status, body).await,
                "remove friend",
                &[200, 204],
            )
            .await
            .unwrap_err();
            assert_eq!(error.status().unwrap().as_u16(), status);
            assert_eq!(error.social_code(), code);
            assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
        }
    }

    #[tokio::test]
    async fn malformed_people_response_is_safe() {
        let error = crate::error::response_json::<SocialSummary>(
            response(200, r#"{"people":[{"xuid":"1","gamertag":"test","displayName":"test","isFollowedByCaller":"ECHOED_SECRET"}]}"#).await,
            "get friends",
        ).await.unwrap_err();
        assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
    }

    #[tokio::test]
    async fn legacy_relationship_accepts_both_upstream_success_codes() {
        for status in [200, 204] {
            FriendsClient::relationship_result(
                response(status, "").await,
                "remove friend",
                &[200, 204],
            )
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn session_and_gallery_json_errors_are_safe() {
        for status in [200, 401, 403, 429, 500] {
            let session = crate::error::response_json::<crate::session::CreateSessionResponse>(
                response(status, r#"{"members":"ECHOED_SECRET"}"#).await,
                "create session",
            )
            .await
            .unwrap_err();
            let gallery = crate::error::response_json::<crate::gallery::GalleryUploadResponse>(
                response(status, r#"{"result":"ECHOED_SECRET"}"#).await,
                "upload gallery image",
            )
            .await
            .unwrap_err();
            for error in [session, gallery] {
                if status != 200 {
                    assert_eq!(error.status().unwrap().as_u16(), status);
                }
                assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
            }
        }
    }
}

#[cfg(test)]
mod response_regressions {
    use super::*;
    use std::io::{Read, Write};

    async fn raw_response(reply: &'static [u8]) -> reqwest::Response {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert_ne!(stream.read(&mut request).unwrap(), 0);
            stream.write_all(reply).unwrap();
        });
        let result = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        server.join().unwrap();
        result
    }

    #[tokio::test]
    async fn truncated_success_body_returns_safe_error() {
        let response = raw_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nECHOED_SECRET",
        )
        .await;
        let error = response_json::<SocialSummary>(response, "get friends")
            .await
            .unwrap_err();
        assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
    }

    #[tokio::test]
    async fn malformed_error_body_does_not_hide_remove_failure() {
        let response = raw_response(b"HTTP/1.1 500 Server Error\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nECHOED_SECRET").await;
        let error = FriendsClient::relationship_result(response, "remove friend", &[200, 204])
            .await
            .unwrap_err();
        assert_eq!(
            error.status(),
            Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR)
        );
        assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
    }

    #[tokio::test]
    async fn retry_after_survives_without_remote_descriptions() {
        let response = raw_response(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 13\r\nConnection: close\r\n\r\nECHOED_SECRET").await;
        let error = FriendsClient::relationship_result(response, "add friend", &[200, 204])
            .await
            .unwrap_err();
        assert_eq!(error.retry_after(), Some(120));
        assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
    }
}

#[cfg(test)]
mod relationship_completeness_tests {
    use super::*;

    #[test]
    fn incomplete_relationships_are_rejected_before_sync() {
        for person in [
            serde_json::json!({"xuid":"1", "isFollowedByCaller":true}),
            serde_json::json!({"xuid":"1", "isFollowingCaller":true}),
            serde_json::json!({"isFollowedByCaller":true, "isFollowingCaller":true}),
        ] {
            let response = serde_json::json!({"people":[person]});
            assert!(serde_json::from_value::<SocialSummary>(response).is_err());
        }
        let summary: SocialSummary = serde_json::from_value(serde_json::json!({
            "people":[{"xuid":"1", "isFollowedByCaller":true, "isFollowingCaller":false}]
        }))
        .unwrap();
        assert_eq!(summary.people[0].gamertag, "");
        assert!(!summary.people[0].is_following_caller);
    }
}
