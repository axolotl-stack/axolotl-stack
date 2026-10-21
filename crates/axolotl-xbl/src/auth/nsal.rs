//! NSAL endpoint discovery, following xal/nsal/title.go and resolver.go.
//! Successful title data is retained for this client; failed loads back off for
//! fifteen seconds. The caller supplies its Xbox token and associated proof key.

use std::borrow::Cow;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::HeaderMap;
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use super::signing::{SigningKeyPair, update_server_time_from_header};
use super::xbl_token::XblToken;
use crate::error::{XblError, XblResult, response_json};

const XBL_RELYING_PARTY: &str = "http://xboxlive.com";
const FAILURE_BACKOFF: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase", default)]
pub(crate) struct SignaturePolicy {
    pub(crate) version: u32,
    pub(crate) max_body_bytes: usize,
    pub(crate) extra_headers: Vec<String>,
}

pub(crate) const AUTH_POLICY: SignaturePolicy = SignaturePolicy {
    version: 1,
    max_body_bytes: 0,
    extra_headers: Vec::new(),
};

impl SignaturePolicy {
    fn body_to_sign<'a>(&self, body: &'a [u8]) -> &'a [u8] {
        if self.max_body_bytes == 0 {
            body
        } else {
            &body[..body.len().min(self.max_body_bytes)]
        }
    }

    pub(crate) fn sign_request(
        &self,
        key: &SigningKeyPair,
        method: &str,
        path: &str,
        authorization: &str,
        body: &[u8],
    ) -> String {
        self.sign_request_with_headers(key, method, path, authorization, body, &HeaderMap::new())
    }

    pub(crate) fn sign_request_with_headers(
        &self,
        key: &SigningKeyPair,
        method: &str,
        path: &str,
        authorization: &str,
        body: &[u8],
        headers: &HeaderMap,
    ) -> String {
        let values: Vec<&str> = self
            .extra_headers
            .iter()
            .map(|name| {
                headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
            })
            .collect();
        key.sign_request_with_policy(
            method,
            path,
            authorization,
            self.body_to_sign(body),
            self.version,
            &values,
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedEndpoint {
    pub(crate) relying_party: String,
    pub(crate) policy: SignaturePolicy,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct TitleData {
    #[serde(rename = "EndPoints")]
    endpoints: Vec<Endpoint>,
    signature_policies: Vec<SignaturePolicy>,
}

impl TitleData {
    // title.go:TitleData.Match: first exact match, otherwise last match.
    fn resolve(&self, url: &Url) -> Option<ResolvedEndpoint> {
        let mut found = None;
        for endpoint in &self.endpoints {
            if !endpoint.matches(url) {
                continue;
            }
            found = Some(endpoint);
            if endpoint.host_type == "fqdn" {
                break;
            }
        }
        found.map(|endpoint| ResolvedEndpoint {
            relying_party: endpoint.relying_party.clone(),
            policy: endpoint
                .signature_policy_index
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| self.signature_policies.get(index))
                .cloned()
                .unwrap_or(AUTH_POLICY),
        })
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct Endpoint {
    protocol: String,
    host: String,
    host_type: String,
    port: u16,
    path: String,
    relying_party: String,
    signature_policy_index: Option<i64>,
}

impl Endpoint {
    fn matches(&self, url: &Url) -> bool {
        if self.relying_party.is_empty() || self.protocol != url.scheme() {
            return false;
        }
        let host = url
            .host_str()
            .unwrap_or("")
            .trim_start_matches('[')
            .trim_end_matches(']');
        let matches_host = match self.host_type.as_str() {
            "fqdn" => self.host == host,
            "wildcard" => wildcard_matches(&self.host, host),
            "cidr" => cidr_matches(&self.host, host),
            _ => false,
        };
        matches_host
            && (self.port == 0
                || Some(self.port)
                    == url.port().or_else(|| match url.scheme() {
                        "https" => Some(443),
                        "http" => Some(80),
                        _ => None,
                    }))
            && (self.path.is_empty() || self.path.as_bytes() == decoded_path(url.path()).as_ref())
    }
}

// Like Go's regexp.QuoteMeta followed by replacing every '*' with '.*',
// without compiling a regular expression for each candidate.
fn wildcard_matches(pattern: &str, host: &str) -> bool {
    if !pattern.starts_with('*') {
        return false;
    }
    let mut remaining = host;
    let mut parts = pattern.split('*').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return remaining.ends_with(part);
        }
        let Some(position) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[position + part.len()..];
    }
    false
}

fn cidr_matches(cidr: &str, host: &str) -> bool {
    let Some((network, bits)) = cidr.split_once('/') else {
        return false;
    };
    let (Ok(network), Ok(host), Ok(bits)) = (
        network.parse::<IpAddr>(),
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>(),
        bits.parse::<u32>(),
    ) else {
        return false;
    };
    match (network, host) {
        (IpAddr::V4(network), IpAddr::V4(host)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(network) & mask == u32::from(host) & mask
        }
        (IpAddr::V6(network), IpAddr::V6(host)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(network) & mask == u128::from(host) & mask
        }
        (IpAddr::V4(network), IpAddr::V6(host)) => host.to_ipv4_mapped().is_some_and(|host| {
            if bits > 32 {
                return false;
            }
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(network) & mask == u32::from(host) & mask
        }),
        _ => false,
    }
}

// Go url.URL.Path is percent-decoded; url::Url::path is not.
fn decoded_path(path: &str) -> Cow<'_, [u8]> {
    if !path.contains('%') {
        return Cow::Borrowed(path.as_bytes());
    }
    let mut bytes = Vec::with_capacity(path.len());
    let input = path.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' && index + 2 < input.len() {
            let high = (input[index + 1] as char).to_digit(16);
            let low = (input[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                bytes.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        bytes.push(input[index]);
        index += 1;
    }
    Cow::Owned(bytes)
}

#[derive(Default)]
struct TitleCache {
    data: Option<Arc<TitleData>>,
    failed_until: Option<Instant>,
}

pub(crate) struct NsalClient {
    client: reqwest::Client,
    base: String,
    default: Mutex<TitleCache>,
    current: Mutex<TitleCache>,
}

impl NsalClient {
    pub(crate) fn new(client: reqwest::Client) -> Self {
        Self::with_base(client, "https://title.mgt.xboxlive.com".into())
    }

    fn with_base(client: reqwest::Client, base: String) -> Self {
        Self {
            client,
            base,
            default: Mutex::default(),
            current: Mutex::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(client: reqwest::Client, base: String) -> Self {
        Self::with_base(client, base)
    }

    /// Public title discovery for pre-authentication requests such as SISU.
    pub(crate) async fn resolve_default(&self, url: &Url) -> XblResult<ResolvedEndpoint> {
        self.title(None).await?.resolve(url).ok_or_else(no_endpoint)
    }

    /// The supplied token must be for the Xbox Live relying party, and the key
    /// must be the proof key that was used to acquire that token.
    pub(crate) async fn resolve(
        &self,
        url: &Url,
        token: &XblToken,
        key: &SigningKeyPair,
    ) -> XblResult<ResolvedEndpoint> {
        let current_failed = match self.title(Some((token, key))).await {
            Ok(title) => {
                if let Some(endpoint) = title.resolve(url) {
                    return Ok(endpoint);
                }
                false
            }
            Err(_) => true,
        };
        let endpoint = self
            .title(None)
            .await?
            .resolve(url)
            .ok_or_else(no_endpoint)?;
        // resolver.go:Resolve: never select a possibly-wrong title service RP
        // from a lower-precedence source after the current title failed.
        if current_failed && endpoint.relying_party != XBL_RELYING_PARTY {
            return Err(no_endpoint());
        }
        Ok(endpoint)
    }

    async fn title(&self, auth: Option<(&XblToken, &SigningKeyPair)>) -> XblResult<Arc<TitleData>> {
        // Holding the async mutex across the load implements single-flight.
        // Cancellation drops it without caching failure, allowing the next caller
        // to retry (resolver.go:titleLoadCanceled).
        let mut cache = if auth.is_some() {
            self.current.lock().await
        } else {
            self.default.lock().await
        };
        if let Some(title) = &cache.data {
            return Ok(Arc::clone(title));
        }
        if cache
            .failed_until
            .is_some_and(|until| Instant::now() < until)
        {
            return Err(XblError::Other("NSAL title data load failed".into()));
        }
        match self.load_title(auth).await {
            Ok(title) => {
                let title = Arc::new(title);
                cache.data = Some(Arc::clone(&title));
                cache.failed_until = None;
                Ok(title)
            }
            Err(error) => {
                cache.failed_until = Some(Instant::now() + FAILURE_BACKOFF);
                Err(error)
            }
        }
    }

    async fn load_title(&self, auth: Option<(&XblToken, &SigningKeyPair)>) -> XblResult<TitleData> {
        let path = if auth.is_some() {
            "/titles/current/endpoints"
        } else {
            "/titles/default/endpoints?type=1"
        };
        let mut request = self
            .client
            .get(format!("{}{path}", self.base))
            .header("x-xbl-contract-version", "1");
        if let Some((token, key)) = auth {
            let authorization = token.auth_header();
            let signature = AUTH_POLICY.sign_request(key, "GET", path, &authorization, &[]);
            request = request
                .header("Authorization", authorization)
                .header("Signature", signature);
        }
        let response = request.send().await?;
        if let Some(date) = response
            .headers()
            .get("Date")
            .and_then(|date| date.to_str().ok())
        {
            update_server_time_from_header(date);
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(XblError::HttpStatus {
                status: response.status(),
                operation: "NSAL title discovery",
            });
        }
        response_json(response, "NSAL title discovery").await
    }
}

fn no_endpoint() -> XblError {
    XblError::Other("NSAL endpoint could not be resolved".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn endpoint(host: &str, kind: &str, rp: &str) -> serde_json::Value {
        json!({"Protocol":"https","Host":host,"HostType":kind,"RelyingParty":rp})
    }

    #[test]
    fn first_fqdn_wins_and_last_wildcard_wins() {
        let url = Url::parse("https://a.example.com/path").unwrap();
        let title: TitleData = serde_json::from_value(json!({"EndPoints":[
            endpoint("*.example.com","wildcard","wild1"),
            endpoint("*example.com","wildcard","wild2")
        ]}))
        .unwrap();
        assert_eq!(title.resolve(&url).unwrap().relying_party, "wild2");
        let title: TitleData = serde_json::from_value(json!({"EndPoints":[
            endpoint("*.example.com","wildcard","wild"),
            endpoint("a.example.com","fqdn","exact1"),
            endpoint("a.example.com","fqdn","exact2")
        ]}))
        .unwrap();
        assert_eq!(title.resolve(&url).unwrap().relying_party, "exact1");
    }

    #[test]
    fn matching_boundaries_and_policy_indices() {
        let mut value = endpoint("*.example.com", "wildcard", "rp");
        value["Port"] = json!(443);
        value["Path"] = json!("/a b");
        for index in [json!(null), json!(-1), json!(1), json!(999)] {
            value["SignaturePolicyIndex"] = index;
            let title: TitleData = serde_json::from_value(
                json!({"EndPoints":[value],"SignaturePolicies":[{"Version":2,"MaxBodyBytes":3}]}),
            )
            .unwrap();
            assert_eq!(
                title
                    .resolve(&Url::parse("https://a.example.com/a%20b?q=1").unwrap())
                    .unwrap()
                    .policy,
                AUTH_POLICY
            );
            for url in [
                "http://a.example.com/a%20b",
                "https://example.com/a%20b",
                "https://a.example.com:444/a%20b",
                "https://a.example.com/a%20b/other",
            ] {
                assert!(title.resolve(&Url::parse(url).unwrap()).is_none(), "{url}");
            }
        }
        value["SignaturePolicyIndex"] = json!(0);
        let title: TitleData = serde_json::from_value(
            json!({"EndPoints":[value],"SignaturePolicies":[{"Version":2,"MaxBodyBytes":3}]}),
        )
        .unwrap();
        assert_eq!(
            title
                .resolve(&Url::parse("https://a.example.com/a%20b").unwrap())
                .unwrap()
                .policy
                .version,
            2
        );
        assert!(wildcard_matches("*foo*bar", "foo123bar"));
        assert!(!wildcard_matches("foo*", "foobar"));
        assert!(cidr_matches("10.0.0.0/8", "10.255.1.2"));
        assert!(!cidr_matches("10.0.0.0/8", "11.0.0.1"));
        assert!(cidr_matches("2001:db8::/32", "[2001:db8::1]"));
        assert!(!cidr_matches("10.0.0.0/33", "10.0.0.1"));
    }

    #[test]
    fn policy_body_boundaries() {
        let body = b"abcdef";
        for (limit, expected) in [
            (0, &body[..]),
            (1, &body[..1]),
            (6, &body[..]),
            (7, &body[..]),
        ] {
            let policy = SignaturePolicy {
                version: 2,
                max_body_bytes: limit,
                extra_headers: vec![],
            };
            assert_eq!(policy.body_to_sign(body), expected);
        }
    }

    async fn server(
        responses: Vec<(u16, serde_json::Value)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buf = [0; 2048];
                    let read = socket.read(&mut buf).await.unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&buf[..read]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        (base, task)
    }

    fn token() -> XblToken {
        XblToken {
            token: "test-token".into(),
            gamertag: String::new(),
            xuid: String::new(),
            user_hash: "hash".into(),
            expires_at: 0,
        }
    }

    #[tokio::test]
    async fn public_default_is_unauthenticated_and_cached() {
        let (base, requests) = server(vec![(
            200,
            json!({"EndPoints":[endpoint("sisu.example.com","fqdn","sisu")]}),
        )])
        .await;
        let client = NsalClient::with_base(reqwest::Client::new(), base);
        let url = Url::parse("https://sisu.example.com/authorize").unwrap();
        for _ in 0..2 {
            assert_eq!(
                client.resolve_default(&url).await.unwrap().relying_party,
                "sisu"
            );
        }
        let requests = requests.await.unwrap();
        assert!(requests[0].starts_with("GET /titles/default/endpoints?type=1 "));
        let headers = requests[0].to_ascii_lowercase();
        assert!(!headers.contains("authorization:"));
        assert!(!headers.contains("signature:"));
        assert!(headers.contains("x-xbl-contract-version: 1"));
    }

    #[tokio::test]
    async fn current_is_signed_and_precedes_default() {
        let (base, requests) = server(vec![(
            200,
            json!({"EndPoints":[endpoint("api.example.com","fqdn","title-rp")]}),
        )])
        .await;
        let client = NsalClient::with_base(reqwest::Client::new(), base);
        let key = SigningKeyPair::generate();
        let url = Url::parse("https://api.example.com/").unwrap();
        for _ in 0..2 {
            assert_eq!(
                client
                    .resolve(&url, &token(), &key)
                    .await
                    .unwrap()
                    .relying_party,
                "title-rp"
            );
        }
        let requests = requests.await.unwrap();
        assert!(requests[0].starts_with("GET /titles/current/endpoints "));
        let headers = requests[0].to_ascii_lowercase();
        assert!(headers.contains("authorization: xbl3.0 x=hash;test-token"));
        assert!(headers.contains("signature:"));
    }

    #[tokio::test]
    async fn unmatched_current_falls_back_to_default() {
        let (base, requests) = server(vec![
            (200, json!({"EndPoints":[]})),
            (
                200,
                json!({"EndPoints":[endpoint("api.example.com","fqdn","default-title-rp")]}),
            ),
        ])
        .await;
        let client = NsalClient::with_base(reqwest::Client::new(), base);
        let key = SigningKeyPair::generate();
        let url = Url::parse("https://api.example.com/").unwrap();
        assert_eq!(
            client
                .resolve(&url, &token(), &key)
                .await
                .unwrap()
                .relying_party,
            "default-title-rp"
        );
        let requests = requests.await.unwrap();
        assert!(requests[0].starts_with("GET /titles/current/endpoints "));
        assert!(requests[1].starts_with("GET /titles/default/endpoints?type=1 "));
        assert!(!requests[1].to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn invalid_title_data_does_not_leak_response() {
        let (base, requests) = server(vec![(200, json!("private-response-secret"))]).await;
        let client = NsalClient::with_base(reqwest::Client::new(), base);
        let error = client
            .resolve_default(&Url::parse("https://api.example.com/").unwrap())
            .await
            .unwrap_err();
        assert!(!format!("{error:?} {error}").contains("private-response-secret"));
        requests.await.unwrap();
    }

    #[tokio::test]
    async fn failed_current_allows_only_generic_default_and_caches_failure() {
        for (rp, succeeds) in [(XBL_RELYING_PARTY, true), ("title-rp", false)] {
            let (base, requests) = server(vec![
                (500, json!({"secret":"not surfaced"})),
                (
                    200,
                    json!({"EndPoints":[endpoint("api.example.com","fqdn",rp)]}),
                ),
            ])
            .await;
            let client = NsalClient::with_base(reqwest::Client::new(), base);
            let key = SigningKeyPair::generate();
            let url = Url::parse("https://api.example.com/").unwrap();
            for _ in 0..2 {
                assert_eq!(client.resolve(&url, &token(), &key).await.is_ok(), succeeds);
            }
            assert_eq!(requests.await.unwrap().len(), 2);
        }
    }
}
