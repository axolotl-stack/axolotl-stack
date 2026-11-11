//! Lockstep play for several clients in one process (`--also NAME`).
//!
//! A single process lets the sweep plugin decide which packets of which
//! client reach BDS in the same server tick, in which order, and whether
//! one client's packets share a game batch. Once the plugin sends the first
//! client a `bds-sweep:tick` line, every session stops its own 50 ms input
//! timer and sends only what the tick lines ask for:
//!
//! ```text
//! bds-sweep:tick ID STEP STEP ... ;
//! ```
//!
//! The steps run in order, each one written to the socket before the next
//! starts:
//!
//! * `iN`: client `N`'s next `PlayerAuthInput` (its simulated move);
//! * `aN>M`: client `N` attacks client `M` with its held item;
//! * `oN=ORDER`: apply a chat order to client `N` (`:` stands for a space,
//!   `o0=sprint:on`), sending whatever it sends at once;
//! * `[STEP,STEP,...]`: steps of one client sent as one game batch.
//!
//! Clients are numbered in login order: 0 is `--name`, then each `--also`.
//! The plugin sends a tick line early in a server tick, and BDS handles the
//! client packets that reach it in the next few milliseconds in that same
//! tick (an immediate answer landed there for 124 of 158 lines, the rest
//! one tick later; with 10 ms more it was 116 of 158, so no delay makes it
//! certain). The steps go out `--lockstep-delay-ms` after the line arrived
//! (0 by default), timed by spinning since a Windows sleep rounds up to
//! about 15 ms. The sweep job marks each line's inputs with a rotation step,
//! so the tick BDS consumed every input in is measured, not assumed.

use anyhow::{Context, Result, bail, ensure};
use tokio::sync::{mpsc, oneshot};

use crate::{Order, parse_order};

/// One thing a client does in a lockstep tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// The next `PlayerAuthInput`.
    Input,
    /// Attack the client with this login index.
    Attack(usize),
    /// A chat order, applied at once.
    Order(Order),
}

/// Acts of one client, sent one batch each or all in one batch.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub client: usize,
    pub acts: Vec<Act>,
    pub batch: bool,
}

/// A parsed `bds-sweep:tick` line.
#[derive(Debug, Clone, PartialEq)]
pub struct TickScript {
    pub id: u64,
    pub steps: Vec<Step>,
}

/// A tick line on its way to the coordinator, with when it arrived.
#[derive(Debug)]
pub struct Line {
    pub script: TickScript,
    pub arrived: std::time::Instant,
}

/// What a session is asked to send, with the target runtime ids resolved.
#[derive(Debug)]
pub struct Control {
    pub acts: Vec<SessionAct>,
    pub batch: bool,
    pub done: oneshot::Sender<Result<()>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionAct {
    Input,
    Attack(u64),
    Order(Order),
}

fn act(word: &str) -> Result<(usize, Act)> {
    let client = |digits: &str| -> Result<usize> {
        digits
            .parse()
            .with_context(|| format!("bad client index in {word:?}"))
    };
    if let Some(rest) = word.strip_prefix('i') {
        return Ok((client(rest)?, Act::Input));
    }
    if let Some(rest) = word.strip_prefix('a') {
        let (from, to) = rest
            .split_once('>')
            .with_context(|| format!("attack without a target: {word:?}"))?;
        return Ok((client(from)?, Act::Attack(client(to)?)));
    }
    if let Some(rest) = word.strip_prefix('o') {
        let (who, order) = rest
            .split_once('=')
            .with_context(|| format!("order without text: {word:?}"))?;
        let text = format!("bds-sweep:{}", order.replace(':', " "));
        let order = parse_order(&text).with_context(|| format!("unknown order {text:?}"))?;
        return Ok((client(who)?, Act::Order(order)));
    }
    bail!("unknown lockstep step {word:?}")
}

/// Parses a `bds-sweep:tick` line out of chat text; `None` for other text.
pub fn parse_tick(text: &str) -> Option<Result<TickScript>> {
    let rest = &text[text.find("bds-sweep:tick ")? + "bds-sweep:tick ".len()..];
    Some(parse_steps(rest))
}

fn parse_steps(rest: &str) -> Result<TickScript> {
    let mut words = rest.split_whitespace();
    let id = words
        .next()
        .context("tick line without an id")?
        .parse()
        .context("tick id")?;
    let mut steps = Vec::new();
    for word in words {
        if word.starts_with(';') {
            return Ok(TickScript { id, steps });
        }
        if let Some(inner) = word.strip_prefix('[') {
            let inner = inner
                .strip_suffix(']')
                .with_context(|| format!("unclosed batch {word:?}"))?;
            let mut client = None;
            let mut acts = Vec::new();
            for part in inner.split(',') {
                let (who, act) = act(part)?;
                ensure!(
                    client.is_none_or(|c| c == who),
                    "a batch holds one client's steps: {word:?}"
                );
                client = Some(who);
                acts.push(act);
            }
            steps.push(Step {
                client: client.context("empty batch")?,
                acts,
                batch: true,
            });
        } else {
            let (client, act) = act(word)?;
            steps.push(Step {
                client,
                acts: vec![act],
                batch: false,
            });
        }
    }
    bail!("tick line without its closing ';'")
}

/// Runs each tick script against the sessions: one step at a time, each
/// acknowledged by its session (written out) before the next is sent.
/// `runtime_ids[n]` is client `n`'s runtime id once it has spawned.
pub async fn coordinate(
    mut lines: mpsc::UnboundedReceiver<Line>,
    sessions: Vec<mpsc::UnboundedSender<Control>>,
    runtime_ids: std::sync::Arc<std::sync::Mutex<Vec<Option<u64>>>>,
    delay: std::time::Duration,
) -> Result<()> {
    while let Some(Line { script, arrived }) = lines.recv().await {
        let due = arrived + delay;
        // Only a positive delay can be missed.
        let late = !delay.is_zero() && std::time::Instant::now() > due;
        while std::time::Instant::now() < due {
            std::hint::spin_loop();
        }
        let started = std::time::Instant::now();
        for step in &script.steps {
            let session = sessions
                .get(step.client)
                .with_context(|| format!("no client {}", step.client))?;
            let acts = step
                .acts
                .iter()
                .map(|act| {
                    Ok(match act {
                        Act::Input => SessionAct::Input,
                        Act::Order(order) => SessionAct::Order(order.clone()),
                        Act::Attack(target) => SessionAct::Attack(
                            runtime_ids
                                .lock()
                                .map_err(|_| anyhow::anyhow!("runtime ids poisoned"))?
                                .get(*target)
                                .copied()
                                .flatten()
                                .with_context(|| format!("client {target} has not spawned"))?,
                        ),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let (done, sent) = oneshot::channel();
            if session
                .send(Control {
                    acts,
                    batch: step.batch,
                    done,
                })
                .is_err()
            {
                bail!("client {} has left", step.client);
            }
            sent.await.context("session dropped a step")??;
        }
        tracing::info!(
            tick = script.id,
            steps = script.steps.len(),
            late,
            micros = started.elapsed().as_micros() as u64,
            "lockstep tick sent"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_lines_parse_inputs_attacks_orders_and_batches() {
        let script =
            parse_tick(r#"{ message: "bds-sweep:tick 42 i1 a0>1 [i0,a0>1,a0>2] o0=sprint:on ;" }"#)
                .unwrap()
                .unwrap();
        assert_eq!(script.id, 42);
        assert_eq!(
            script.steps,
            vec![
                Step {
                    client: 1,
                    acts: vec![Act::Input],
                    batch: false
                },
                Step {
                    client: 0,
                    acts: vec![Act::Attack(1)],
                    batch: false
                },
                Step {
                    client: 0,
                    acts: vec![Act::Input, Act::Attack(1), Act::Attack(2)],
                    batch: true
                },
                Step {
                    client: 0,
                    acts: vec![Act::Order(Order::Sprint(true))],
                    batch: false
                },
            ]
        );
    }

    #[test]
    fn orders_take_colons_for_spaces_and_bad_lines_fail() {
        let script = parse_tick("bds-sweep:tick 1 o2=look:-90:12.5 o1=move:1:0 ;")
            .unwrap()
            .unwrap();
        assert_eq!(
            script.steps[0].acts,
            vec![Act::Order(Order::Look {
                yaw: -90.0,
                pitch: 12.5
            })]
        );
        assert_eq!(script.steps[0].client, 2);
        assert!(parse_tick("bds-sweep:tick 1 i0").unwrap().is_err());
        assert!(parse_tick("bds-sweep:tick 1 [i0,i1] ;").unwrap().is_err());
        assert!(parse_tick("bds-sweep:tick 1 x0 ;").unwrap().is_err());
        assert!(parse_tick("bds-sweep:move 1 0").is_none());
    }
}
