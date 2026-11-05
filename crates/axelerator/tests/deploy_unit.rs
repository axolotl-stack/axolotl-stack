//! Checks on the shipped systemd unit (`deploy/axelerator.service`).

const UNIT: &str = include_str!("../deploy/axelerator.service");

/// The value of `key` in `[section]`, ignoring comments.
fn setting(section: &str, key: &str) -> Option<&'static str> {
    let mut current = "";
    for line in UNIT.lines().map(str::trim) {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name;
            continue;
        }
        if current == section
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim());
        }
    }
    None
}

/// Failures exit non-zero, so systemd restarts the gateway. With the old
/// `StartLimitBurst=5` in 300 s and `RestartSec=5`, an outage longer than
/// about 25 s used up the start limit and left the unit failed until
/// someone started it by hand. Restarts must never give up, back off
/// instead, and a requested stop (exit 0) must still not restart.
#[test]
fn unit_keeps_restarting_with_backoff() {
    assert_eq!(setting("Service", "Restart"), Some("on-failure"));
    assert_eq!(setting("Service", "RestartPreventExitStatus"), Some("0"));
    // The start rate limit lives in [Unit]; 0 disables it.
    assert_eq!(setting("Unit", "StartLimitIntervalSec"), Some("0"));
    assert_eq!(setting("Service", "StartLimitIntervalSec"), None);
    assert_eq!(setting("Service", "StartLimitBurst"), None);
    // Growing delay between restarts (systemd 254+).
    let steps: u32 = setting("Service", "RestartSteps")
        .expect("RestartSteps")
        .parse()
        .unwrap();
    assert!(steps > 0);
    assert!(setting("Service", "RestartMaxDelaySec").is_some());
    assert!(setting("Service", "RestartSec").is_some());
}
