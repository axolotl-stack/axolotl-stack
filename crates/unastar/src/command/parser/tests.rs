//! Parser tests over the captured overload shapes.

use super::*;
use crate::command::args::Args;
use unastar_data::commands::{CommandOverload, CommandParam};

fn schema() -> CommandSchema {
    CommandSchema::vanilla()
}

/// A token that fits `param`, written the way a player would type it.
fn sample(schema: &CommandSchema, param: &CommandParam) -> String {
    match &param.kind {
        ParamKind::Basic(basic) => match basic {
            BasicType::Int => "3".into(),
            BasicType::Float => "1.5".into(),
            BasicType::Value => "~10".into(),
            BasicType::WildcardInt => "*".into(),
            BasicType::Operator => "+=".into(),
            BasicType::CompareOperator => "<=".into(),
            BasicType::Target => "@e[type=cow,r=5]".into(),
            BasicType::WildcardTarget => "*".into(),
            BasicType::FilePath => "path/to/thing".into(),
            BasicType::IntegerRange => "1..5".into(),
            BasicType::String => "\"two words\"".into(),
            BasicType::BlockPosition => "1 ~2 -3".into(),
            BasicType::Position => "^ ^1 ^2.5".into(),
            BasicType::Message | BasicType::RawText => "hello there @s".into(),
            BasicType::Json => "{\"rawtext\":[{\"text\":\"hi\"}]}".into(),
            BasicType::BlockStates => "[\"color\"=\"red\",\"age\"=3,\"open_bit\"=true]".into(),
            BasicType::Command => "say hi".into(),
        },
        ParamKind::Enum(name) => schema.syntax().enum_named(name).unwrap().values[0].clone(),
        ParamKind::SoftEnum(name) => schema
            .syntax()
            .soft_enum_named(name)
            .unwrap()
            .values
            .first()
            .cloned()
            .unwrap_or_else(|| "anything".into()),
        ParamKind::Postfix(postfix) => format!("5{}", postfix.to_uppercase()),
        ParamKind::Chained(_) => "as @s run say hi".into(),
    }
}

/// A line for `overload` with its first `count` parameters.
fn line(schema: &CommandSchema, name: &str, overload: &CommandOverload, count: usize) -> String {
    let mut line = name.to_owned();
    for param in &overload.params[..count] {
        line.push(' ');
        line.push_str(&sample(schema, param));
    }
    line
}

fn parse_ok(line: &str) -> Args {
    parse(&schema(), line).unwrap_or_else(|e| panic!("{line}: {e:?}"))
}

fn syntax_error(line: &str) -> (String, String, String) {
    match parse(&schema(), line) {
        Err(ParseError::Syntax(e)) => (e.before, e.token, e.after),
        other => panic!("{line}: expected a syntax error, got {other:?}"),
    }
}

#[test]
fn every_captured_overload_parses_with_and_without_optional_parameters() {
    let schema = schema();
    let mut checked = 0;
    for command in &schema.syntax().commands {
        for (index, overload) in command.overloads.iter().enumerate() {
            let required = overload.params.iter().take_while(|p| !p.optional).count();
            let mut counts = vec![required, overload.params.len()];
            counts.dedup();
            for count in counts {
                // Message-like parameters consume the rest of the line.
                if overload.params[..count.saturating_sub(1)].iter().any(|p| {
                    matches!(
                        p.kind,
                        ParamKind::Basic(
                            BasicType::Message | BasicType::RawText | BasicType::Command
                        ) | ParamKind::Chained(_)
                    )
                }) {
                    continue;
                }
                let text = line(&schema, &command.name, overload, count);
                let args = parse(&schema, &text)
                    .unwrap_or_else(|e| panic!("/{text} (overload {index}): {e:?}"));
                // An earlier overload may accept the same line; the matched one
                // must still have read every parameter we wrote.
                assert!(args.overload() <= index, "/{text}");
                if args.overload() == index {
                    for param in &overload.params[..count] {
                        assert!(args.has(&param.name), "/{text}: {} missing", param.name);
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 300, "only {checked} lines");
}

#[test]
fn typed_values_come_back_with_their_meaning() {
    let args = parse_ok("give @s minecraft:WHEAT 64 0 {\"minecraft:can_destroy\":{}}");
    assert_eq!(args.get::<&str>("itemName"), Some("minecraft:wheat"));
    assert_eq!(args.get::<i32>("amount"), Some(64));
    assert!(
        args.get::<&serde_json::Value>("components")
            .unwrap()
            .is_object()
    );
    assert!(matches!(
        args.get::<&Target>("player"),
        Some(Target::Selector(_))
    ));

    let args = parse_ok("tp Steve ~1 ~ 5.5 ~90 -10");
    assert_eq!(
        args.get::<&Target>("victim"),
        Some(&Target::Name("Steve".into()))
    );
    assert_eq!(
        args.get::<&CommandPosition>("destination"),
        Some(&CommandPosition([
            Coordinate::Relative(1.0),
            Coordinate::Relative(0.0),
            Coordinate::Absolute(5.5),
        ]))
    );
    let yaw: crate::command::RelativeFloat = args.get("yRot").unwrap();
    assert_eq!(yaw.resolve(10.0), 100.0);

    let args = parse_ok("xp 5L @a");
    assert_eq!(args.overload(), 1);
    assert_eq!(args.get::<i32>("amount"), Some(5));

    let args = parse_ok("setblock ~ ~-1 ~ wool[\"color\"=\"red\"] keep");
    assert_eq!(
        args.get::<&[(String, StateLiteral)]>("blockStates"),
        Some(&[("color".to_owned(), StateLiteral::String("red".into()))][..])
    );
    assert_eq!(args.get::<&str>("oldBlockHandling"), Some("keep"));

    let args = parse_ok("say  hello   world ");
    assert_eq!(args.get::<&str>("message"), Some("hello   world"));
}

#[test]
fn coordinates_follow_bedrock_rules() {
    let position = |text: &str| {
        *parse_ok(&format!("setworldspawn {text}"))
            .get::<&CommandPosition>("spawnPoint")
            .unwrap()
    };
    // Whole-number x/z are block centres; y and decimals are kept.
    assert_eq!(
        position("10 64 -3").resolve(glam::DVec3::ZERO, 0.0, 0.0),
        glam::DVec3::new(10.5, 64.0, -2.5)
    );
    assert_eq!(
        position("~~1~-2").resolve(glam::DVec3::new(1.0, 2.0, 3.0), 0.0, 0.0),
        glam::DVec3::new(1.0, 3.0, 1.0)
    );
    // `^` is left, up, forward: at yaw 0 forward is +z.
    let local = position("^1 ^ ^2").resolve(glam::DVec3::ZERO, 0.0, 0.0);
    assert!(
        (local - glam::DVec3::new(1.0, 0.0, 2.0)).length() < 1e-9,
        "{local}"
    );
    // `^` cannot mix with other kinds.
    assert!(matches!(
        parse(&schema(), "setworldspawn ^ ~ ^"),
        Err(ParseError::Syntax(_))
    ));
}

#[test]
fn syntax_errors_point_at_the_token_bds_reports() {
    assert_eq!(
        syntax_error("gamemode foo"),
        ("/gamemode ".into(), "foo".into(), String::new())
    );
    assert_eq!(
        syntax_error("give @s notanitem 3"),
        ("/give @s ".into(), "notanitem".into(), " 3".into())
    );
    assert_eq!(
        syntax_error("give @s"),
        ("/give @s".into(), String::new(), String::new())
    );
    assert_eq!(
        syntax_error("list extra"),
        ("/list ".into(), "extra".into(), String::new())
    );
    // The furthest point any overload reached wins.
    assert_eq!(
        // (`1` is also a valid player name: overload 4 reads victim 1,
        // destination 2 3 4, rotation 5 6 and then fails at 7.)
        syntax_error("tp 1 2 3 4 5 6 7"),
        ("/tp 1 2 3 4 5 6 ".into(), "7".into(), String::new())
    );
    assert_eq!(
        syntax_error("give @s wheat 1.5"),
        ("/give @s wheat ".into(), "1.5".into(), String::new())
    );
}

#[test]
fn names_resolve_through_aliases_and_unknown_names_are_reported() {
    assert_eq!(parse_ok("TP @s ~ ~ ~").command().name, "teleport");
    assert_eq!(parse_ok("w @a hi").command().name, "tell");
    assert_eq!(
        parse(&schema(), "notacommand 1"),
        Err(ParseError::Unknown("notacommand".into()))
    );
}
