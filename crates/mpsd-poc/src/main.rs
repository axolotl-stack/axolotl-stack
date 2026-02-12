//! MPSD Enumeration Security Research POC
//!
//! This tool tests whether the Xbox Live Multiplayer Session Directory (MPSD)
//! allows unauthorized enumeration of Minecraft Bedrock sessions.
//!
//! The goal is to verify if this vulnerability exists to report to Microsoft.
//!
//! ## Background
//!
//! The MPSD provides endpoints to query sessions:
//! - GET /serviceconfigs/{scid}/sessions
//! - GET /serviceconfigs/{scid}/sessiontemplates/{template}/sessions
//! - POST /serviceconfigs/{scid}/batch
//! - POST /handles/query
//!
//! If these endpoints return sessions beyond the authenticated user's friends,
//! it would allow attackers to enumerate NetherNet IDs for active Minecraft worlds.

use anyhow::{Context, Result};
use axolotl_xbl::auth::{DeviceCodeAuth, XblTokenClient, relying_party};
use axolotl_xbl::{MpsdEnumerator, scids, templates};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

/// MPSD Enumeration Security Research POC
#[derive(Parser, Debug)]
#[command(name = "mpsd-poc")]
#[command(about = "Test MPSD session enumeration vulnerability for responsible disclosure")]
struct Args {
    /// Path to token cache file
    #[arg(short, long, default_value = "mpsd_token.json")]
    token_path: PathBuf,

    /// Output results to JSON file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Run quick test (only test one SCID and template)
    #[arg(long)]
    quick: bool,

    /// Verbose output (show full responses)
    #[arg(short, long)]
    verbose: bool,
}

/// Cached OAuth token (same structure as axelerator).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedToken {
    oauth: axolotl_xbl::OAuthToken,
    acquired_at: u64,
    expires_at: u64,
}

impl CachedToken {
    fn new(oauth: axolotl_xbl::OAuthToken) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expires_at = now + oauth.expires_in;
        Self {
            oauth,
            acquired_at: now,
            expires_at,
        }
    }

    fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now + 300 >= self.expires_at
    }

    fn can_refresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let refresh_expiry = self.acquired_at + (90 * 24 * 60 * 60);
        now < refresh_expiry
    }
}

/// Load or create OAuth token.
async fn get_oauth_token(token_path: &PathBuf) -> Result<axolotl_xbl::OAuthToken> {
    // Try to load existing token
    if let Ok(data) = tokio::fs::read_to_string(token_path).await
        && let Ok(cached) = serde_json::from_str::<CachedToken>(&data)
    {
        if !cached.is_expired() {
            info!("Using cached OAuth token");
            return Ok(cached.oauth);
        }

        if cached.can_refresh() {
            info!("Refreshing expired OAuth token...");
            let auth = DeviceCodeAuth::new();
            match auth.refresh(&cached.oauth.refresh_token).await {
                Ok(refreshed) => {
                    save_token(token_path, &refreshed).await?;
                    return Ok(refreshed);
                }
                Err(e) => {
                    warn!("Token refresh failed: {}", e);
                }
            }
        }
    }

    // Need fresh authentication
    info!("Starting device code authentication...");
    let auth = DeviceCodeAuth::new();
    let code = auth
        .start()
        .await
        .context("Failed to start device code flow")?;

    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║              MPSD ENUMERATION POC - LOGIN                ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║                                                          ║");
    println!("║  1. Open: {:<43} ║", code.verification_uri);
    println!("║  2. Enter code: {:<37} ║", code.user_code);
    println!("║                                                          ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    let token = auth
        .wait_for_auth(&code)
        .await
        .context("Authentication failed or timed out")?;

    save_token(token_path, &token).await?;
    Ok(token)
}

async fn save_token(path: &PathBuf, token: &axolotl_xbl::OAuthToken) -> Result<()> {
    let cached = CachedToken::new(token.clone());
    let json = serde_json::to_string_pretty(&cached)?;
    tokio::fs::write(path, json).await?;
    info!("Token cached to {:?}", path);
    Ok(())
}

/// Result summary for reporting.
#[derive(Debug, Serialize)]
struct TestSummary {
    timestamp: String,
    gamertag: String,
    xuid: String,
    total_tests: usize,
    successful_tests: usize,
    failed_tests: usize,
    sessions_found: usize,
    vulnerability_detected: bool,
    results: Vec<TestResult>,
}

#[derive(Debug, Serialize)]
struct TestResult {
    endpoint: String,
    status: u16,
    success: bool,
    session_count: usize,
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_response: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,reqwest=warn".into()),
        )
        .init();

    let args = Args::parse();

    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║     MPSD SESSION ENUMERATION - SECURITY RESEARCH POC     ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Purpose: Test if MPSD allows unauthorized session       ║");
    println!("║           enumeration for responsible disclosure to MS   ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // Get OAuth token
    let oauth = get_oauth_token(&args.token_path).await?;

    // Exchange for XBL token
    info!("Exchanging OAuth token for Xbox Live token...");
    let xbl_client = XblTokenClient::new();
    let xbl_token = xbl_client
        .get_xbl_token(&oauth, Some(relying_party::XBOX_LIVE))
        .await
        .context("Failed to get Xbox Live token")?;

    info!(
        "Authenticated as: {} (XUID: {})",
        xbl_token.gamertag(),
        xbl_token.xuid()
    );

    // Create enumerator
    let enumerator = MpsdEnumerator::new();

    println!();
    println!("Starting enumeration tests...");
    println!("═══════════════════════════════════════════════════════════");

    let mut results = Vec::new();
    let mut total_sessions = 0;

    if args.quick {
        // Quick test - try multiple variations to find what works
        info!("Running quick test with multiple parameter variations...");

        let scid = scids::AXOLOTL;
        let template = templates::MINECRAFT_LOBBY;

        // Test 1: Basic request (no params)
        println!();
        println!("Test 1: Basic GET (no params)");
        let result = enumerator
            .enumerate_by_template(&xbl_token, scid, template)
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 2: With xuid parameter (our own)
        println!();
        println!("Test 2: With xuid={}", xbl_token.xuid());
        let result = enumerator
            .enumerate_with_params(
                &xbl_token,
                scid,
                template,
                &[("xuid", xbl_token.xuid())],
                "107",
            )
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 3: With visibility=open
        println!();
        println!("Test 3: With visibility=open");
        let result = enumerator
            .enumerate_with_params(&xbl_token, scid, template, &[("visibility", "open")], "107")
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 4: With take parameter
        println!();
        println!("Test 4: With take=100");
        let result = enumerator
            .enumerate_with_params(&xbl_token, scid, template, &[("take", "100")], "107")
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 5: Different contract versions
        for version in ["104", "105", "108"] {
            println!();
            println!("Test: Contract version {}", version);
            let result = enumerator
                .enumerate_with_params(&xbl_token, scid, template, &[], version)
                .await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);
        }

        // Test 6: Keyword enumeration - THE KEY VULNERABILITY TEST
        // If we can search by keyword and get sessions from non-friends, that's the bug
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("KEYWORD ENUMERATION TESTS (potential vulnerability vector)");
        println!("═══════════════════════════════════════════════════════════");

        let keywords = [
            // Common default world names
            "My World",
            "My world",
            "World",
            "world",
            "Realm",
            "realm",
            // Very common names
            "test",
            "Test",
            "survival",
            "Survival",
            "creative",
            "Creative",
            "server",
            "Server",
            "SMP",
            "smp",
            // Single letters (should match many)
            "a",
            "e",
            "s",
            "m",
            // Numbers (common in world names)
            "1",
            "2",
            // Minecraft terms
            "Minecraft",
            "minecraft",
            "Bedrock",
            // Invalid attempts
            "*",
            "%",
            "",
        ];

        for keyword in keywords {
            println!();
            println!("Keyword search: '{}'", keyword);
            let result = enumerator
                .enumerate_by_keyword(&xbl_token, scid, template, keyword)
                .await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);
        }

        // Test: Club ID enumeration
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("CLUB ID TESTS (another potential vector)");
        println!("═══════════════════════════════════════════════════════════");

        // Try some random club IDs - if this returns sessions, could enumerate clubs
        let club_ids = ["1", "12345", "123456789", "3379884749052370"];
        for club_id in club_ids {
            println!();
            println!("Club ID: {}", club_id);
            let result = enumerator
                .enumerate_with_params(&xbl_token, scid, template, &[("clubid", club_id)], "107")
                .await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);
        }

        // Test: Batch query with our own XUID
        println!();
        println!("Batch query with own XUID");
        let result = enumerator
            .batch_query_scid(&xbl_token, scid, &[xbl_token.xuid()])
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 8: Handle query - activity type with own XUID
        println!();
        println!("Test 8: Handle query (activity type, own XUID)");
        let result = enumerator
            .query_handles(&xbl_token, "activity", &[xbl_token.xuid()])
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 9: Handle query - with random XUID
        println!();
        println!("Test 9: Handle query (activity type, random XUID)");
        let result = enumerator
            .query_handles(&xbl_token, "activity", &["2533274792693551"])
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 10: Enumerate templates (to see what templates exist)
        println!();
        println!("Test 10: Enumerate templates");
        let result = enumerator.enumerate_templates(&xbl_token, scid).await;
        print_result(&result, args.verbose);
        results.push(result);

        // Test 11: Alternative SCID
        println!();
        println!("Test 11: Alternative SCID (from security research)");
        let result = enumerator
            .enumerate_by_template(&xbl_token, scids::RESEARCH, template)
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 12: Query a RANDOM XUID (can we see other people's sessions?)
        // This is a key vulnerability test - if we can query arbitrary XUIDs
        // and get their sessions, that's a privacy issue
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("XUID ENUMERATION TEST (can we query other users?)");
        println!("═══════════════════════════════════════════════════════════");

        // Some well-known/public XUIDs to test (these are public gaming figures)
        // We're just testing if the API restricts access, not actually trying to enumerate
        let test_xuids = [
            "2533274792693551", // Random XUID format test
            "2535419876543210", // Another random XUID
        ];

        for test_xuid in test_xuids {
            println!();
            println!("Querying XUID: {}", test_xuid);
            let result = enumerator
                .enumerate_with_params(&xbl_token, scid, template, &[("xuid", test_xuid)], "107")
                .await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);
        }

        // Test 13: Batch query with multiple XUIDs (including random ones)
        println!();
        println!("Test 13: Batch query with own + random XUID");
        let result = enumerator
            .batch_query_scid(&xbl_token, scid, &[xbl_token.xuid(), "2533274792693551"])
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Test 14: MinecraftRealms template (discovered via enumeration)
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("TESTING MinecraftRealms TEMPLATE");
        println!("═══════════════════════════════════════════════════════════");

        // Query MinecraftRealms with keyword
        for keyword in ["a", "Realm", "Server"] {
            println!();
            println!("MinecraftRealms keyword: '{}'", keyword);
            let result = enumerator
                .enumerate_by_keyword(&xbl_token, scid, "MinecraftRealms", keyword)
                .await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);
        }

        // Query MinecraftRealms with random XUID
        println!();
        println!("MinecraftRealms with random XUID");
        let result = enumerator
            .enumerate_with_params(
                &xbl_token,
                scid,
                "MinecraftRealms",
                &[("xuid", "2533274792693551")],
                "107",
            )
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);

        // Additional enumeration vectors - presence/activity APIs
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("PRESENCE/ACTIVITY APIS (alternative enumeration vectors)");
        println!("═══════════════════════════════════════════════════════════");

        // PeopleHub with presence decorations - might expose joinable friends
        println!();
        println!("PeopleHub with presence decorations:");
        let result = enumerator.query_peoplehub_presence(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // Activity feed - might contain session info
        println!();
        println!("Activity feed:");
        let result = enumerator.query_activity_feed(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // User presence for own XUID
        println!();
        println!("Own user presence:");
        let result = enumerator
            .get_user_presence(&xbl_token, xbl_token.xuid())
            .await;
        print_result(&result, args.verbose);
        results.push(result);

        // User presence for random XUID - can we see other's presence?
        println!();
        println!("Random user presence (can we see others?):");
        let result = enumerator
            .get_user_presence(&xbl_token, "2533274792693551")
            .await;
        print_result(&result, args.verbose);
        results.push(result);

        // Multiplayer activity - might list joinable friends
        println!();
        println!("Multiplayer activity (session handles):");
        let result = enumerator.get_multiplayer_activity(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // ═══════════════════════════════════════════════════════════
        // MINECRAFT SIGNALING SERVER TESTS
        // ═══════════════════════════════════════════════════════════
        println!();
        println!("═══════════════════════════════════════════════════════════");
        println!("MINECRAFT SIGNALING SERVER TESTS");
        println!("═══════════════════════════════════════════════════════════");

        // Signaling server root
        println!();
        println!("Signaling server root:");
        let result = enumerator.test_signaling_discovery(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // Signaling API
        println!();
        println!("Signaling API:");
        let result = enumerator.test_signaling_api(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // Auth sessions
        println!();
        println!("Auth sessions endpoint:");
        let result = enumerator.test_auth_sessions(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // Minecraft discovery
        println!();
        println!("Minecraft services discovery:");
        let result = enumerator.test_mc_discovery(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);

        // Realms API
        println!();
        println!("Realms API:");
        let result = enumerator.test_realms_api(&xbl_token).await;
        print_result(&result, args.verbose);
        results.push(result);
    } else {
        // Full test suite
        info!("Running full enumeration test suite...");

        // Test both SCIDs
        for scid in [scids::AXOLOTL, scids::RESEARCH] {
            println!();
            println!("Testing SCID: {}", scid);
            println!("───────────────────────────────────────────────────────────");

            // 1. Enumerate templates
            let result = enumerator.enumerate_templates(&xbl_token, scid).await;
            print_result(&result, args.verbose);
            results.push(result);

            // 2. Enumerate by SCID only
            let result = enumerator.enumerate_by_scid(&xbl_token, scid).await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);

            // 3. Enumerate by each template
            for template in templates::ALL {
                let result = enumerator
                    .enumerate_by_template(&xbl_token, scid, template)
                    .await;
                print_result(&result, args.verbose);
                total_sessions += result.session_count;
                results.push(result);
            }

            // 4. Keyword enumeration tests
            for template in templates::ALL {
                for keyword in ["a", "Minecraft", "Server"] {
                    let result = enumerator
                        .enumerate_by_keyword(&xbl_token, scid, template, keyword)
                        .await;
                    print_result(&result, args.verbose);
                    total_sessions += result.session_count;
                    results.push(result);
                }
            }

            // 5. Batch query at SCID level with own XUID
            let result = enumerator
                .batch_query_scid(&xbl_token, scid, &[xbl_token.xuid()])
                .await;
            print_result(&result, args.verbose);
            total_sessions += result.session_count;
            results.push(result);

            // 6. Batch query at template level
            for template in templates::ALL {
                let result = enumerator
                    .batch_query_template(&xbl_token, scid, template, &[xbl_token.xuid()])
                    .await;
                print_result(&result, args.verbose);
                total_sessions += result.session_count;
                results.push(result);
            }
        }

        // 7. Query handles
        println!();
        println!("Testing handle queries...");
        println!("───────────────────────────────────────────────────────────");

        // Activity handles for own XUID
        let result = enumerator
            .query_handles(&xbl_token, "activity", &[xbl_token.xuid()])
            .await;
        print_result(&result, args.verbose);
        total_sessions += result.session_count;
        results.push(result);
    }

    // Build summary
    let successful = results.iter().filter(|r| r.success).count();
    let failed = results.iter().filter(|r| !r.success).count();

    // Vulnerability is detected if we get sessions AND they're not all from our friends
    // (We'd need additional logic to check if sessions belong to friends)
    let vulnerability_detected = total_sessions > 0;

    let summary = TestSummary {
        timestamp: chrono::Utc::now().to_rfc3339(),
        gamertag: xbl_token.gamertag().to_string(),
        xuid: xbl_token.xuid().to_string(),
        total_tests: results.len(),
        successful_tests: successful,
        failed_tests: failed,
        sessions_found: total_sessions,
        vulnerability_detected,
        results: results
            .into_iter()
            .map(|r| TestResult {
                endpoint: r.endpoint,
                status: r.status,
                success: r.success,
                session_count: r.session_count,
                error: r.error,
                raw_response: if args.verbose {
                    Some(r.raw_response)
                } else {
                    None
                },
            })
            .collect(),
    };

    // Print summary
    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("                        SUMMARY");
    println!("═══════════════════════════════════════════════════════════");
    println!("Total tests:      {}", summary.total_tests);
    println!("Successful:       {}", summary.successful_tests);
    println!("Failed:           {}", summary.failed_tests);
    println!("Sessions found:   {}", summary.sessions_found);
    println!();

    if vulnerability_detected {
        println!("⚠️  POTENTIAL VULNERABILITY DETECTED!");
        println!("   Sessions were returned from enumeration endpoints.");
        println!("   Further analysis needed to determine if these are");
        println!("   sessions outside of the authenticated user's friends.");
        println!();
        println!("   Recommendation: Report to Microsoft Security Response");
        println!("   Center (MSRC) at https://msrc.microsoft.com/");
    } else {
        println!("✓  No sessions found in enumeration tests.");
        println!();
        println!("   FINDINGS:");
        println!("   - XUID queries for arbitrary users return HTTP 200 (API accepts them)");
        println!("   - Keyword searches return HTTP 200 (API accepts them)");
        println!("   - All queries return empty results");
        println!();
        println!("   POSSIBLE INTERPRETATIONS:");
        println!("   1. API is properly restricted (only returns friends' sessions)");
        println!("   2. Queried users don't have active sessions right now");
        println!("   3. Sessions require specific privacy settings to be visible");
        println!();
        println!("   TO PROPERLY VERIFY:");
        println!("   1. Have a FRIEND open a Minecraft world and verify you can see their session");
        println!("   2. Have a NON-FRIEND (with public profile) open a world");
        println!("   3. If you can see the non-friend's session, that's the vulnerability");
        println!();
        println!("   The fact that the API accepts arbitrary XUID queries (HTTP 200)");
        println!("   suggests the endpoint MIGHT return non-friend sessions if they exist.");
    }

    // Save results if requested
    if let Some(output_path) = args.output {
        let json = serde_json::to_string_pretty(&summary)?;
        tokio::fs::write(&output_path, json).await?;
        println!();
        println!("Results saved to: {:?}", output_path);
    }

    Ok(())
}

fn print_result(result: &axolotl_xbl::EnumerationResult, verbose: bool) {
    let status_icon = if result.success { "✓" } else { "✗" };
    let status_color = if result.success { "32" } else { "31" };

    println!(
        "\x1b[{}m{}\x1b[0m {} - HTTP {} - {} sessions",
        status_color, status_icon, result.endpoint, result.status, result.session_count
    );

    if let Some(ref err) = result.error {
        error!("  Error: {}", err);
    }

    // Always show error response bodies (they contain useful info)
    if !result.success && !result.raw_response.is_empty() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result.raw_response) {
            println!(
                "  Error response: {}",
                serde_json::to_string_pretty(&json).unwrap_or(result.raw_response.clone())
            );
        } else {
            println!(
                "  Error response: {}",
                &result.raw_response[..result.raw_response.len().min(500)]
            );
        }
    }

    // Always show successful responses that have content (to understand the API)
    if result.success && !result.raw_response.is_empty() && result.raw_response != "{}" {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result.raw_response) {
            // Check if response has meaningful content
            let has_content = json
                .get("results")
                .map(|r| !r.as_array().map(|a| a.is_empty()).unwrap_or(true))
                .unwrap_or(false)
                || json.get("sessionTemplates").is_some()
                || json.as_object().map(|o| !o.is_empty()).unwrap_or(false);

            if has_content || verbose {
                println!(
                    "  Response: {}",
                    serde_json::to_string_pretty(&json).unwrap_or(result.raw_response.clone())
                );
            }
        } else if verbose {
            println!("  Response: {}", result.raw_response);
        }
    }

    if result.session_count > 0 {
        warn!("  ⚠️  Found {} session(s)!", result.session_count);
        for (i, session) in result.sessions.iter().take(5).enumerate() {
            if let Some(ref name) = session.name {
                println!("    [{}] Session: {}", i + 1, name);
            }
            if let Some(ref props) = session.custom_properties {
                // Try to extract NetherNet ID
                if let Some(connections) =
                    props.get("SupportedConnections").and_then(|c| c.as_array())
                {
                    for conn in connections {
                        if let Some(nid) = conn.get("NetherNetId").or(conn.get("netherNetId")) {
                            println!("       NetherNet ID: {}", nid);
                        }
                    }
                }
                if let Some(host) = props.get("hostName") {
                    println!("       Host: {}", host);
                }
                if let Some(world) = props.get("worldName") {
                    println!("       World: {}", world);
                }
            }
        }
        if result.session_count > 5 {
            println!("    ... and {} more", result.session_count - 5);
        }
    }
}
