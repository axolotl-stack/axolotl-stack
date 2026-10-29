use super::*;
use crate::ParseError;
use serde_json::Value;

type Result<T, E = ParseError> = std::result::Result<T, E>;

fn number(v: &Value, path: &str) -> Result<LootNumber> {
    if let Some(n) = v.as_f64() {
        return Ok(LootNumber::Constant(n as f32));
    }
    if v["type"] == "binomial" {
        return Ok(LootNumber::Binomial {
            n: uint(&v["n"], path)?,
            p: float(&v["p"], path)?,
        });
    }
    Ok(LootNumber::Uniform {
        min: float(&v["min"], path)?,
        max: float(&v["max"], path)?,
    })
}

fn float(v: &Value, path: &str) -> Result<f32> {
    v.as_f64()
        .map(|n| n as f32)
        .ok_or_else(|| ParseError::new(path, format!("expected a number, got {v}")))
}

/// `object[key]` as a number, `default` when absent (BDS `asFloat(default)`).
fn float_or(object: &Value, key: &str, default: f32, path: &str) -> Result<f32> {
    object.get(key).map_or(Ok(default), |v| float(v, path))
}

/// `object[key]` as an integer, `default` when absent (BDS `asInt(default)`).
fn int_or(object: &Value, key: &str, default: i32, path: &str) -> Result<i32> {
    object.get(key).map_or(Ok(default), |v| {
        v.as_i64()
            .and_then(|n| n.try_into().ok())
            .ok_or_else(|| ParseError::new(path, format!("expected i32, got {v}")))
    })
}

fn uint(v: &Value, path: &str) -> Result<u32> {
    v.as_u64()
        .and_then(|n| n.try_into().ok())
        .ok_or_else(|| ParseError::new(path, format!("expected u32, got {v}")))
}

fn text<'a>(v: &'a Value, path: &str) -> Result<&'a str> {
    v.as_str()
        .ok_or_else(|| ParseError::new(path, format!("expected a string, got {v}")))
}

fn array<T>(
    v: Option<&Value>,
    path: &str,
    mut decode: impl FnMut(&Value, &str) -> Result<T>,
) -> Result<Vec<T>> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| decode(v, &format!("{path}/{i}")))
            .collect(),
        Some(other) => Err(ParseError::new(
            path,
            format!("expected an array, got {other}"),
        )),
    }
}

fn bare(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

// The existing evaluator only used these enchantments. Resolve their public
// Bedrock protocol ids once; retain unknown names so evaluation can reject them.
fn enchantment_id(name: &str) -> Option<i16> {
    Some(match bare(name) {
        "aqua_affinity" => 8,
        "looting" => 14,
        "efficiency" => 15,
        "silk_touch" => 16,
        "unbreaking" => 17,
        "fortune" => 18,
        _ => return None,
    })
}

fn condition(v: &Value, path: &str) -> Result<LootCondition> {
    let name = text(&v["condition"], path)?;
    Ok(match bare(name) {
        "random_chance" => LootCondition::RandomChance {
            chance: float(&v["chance"], path)?,
        },
        "random_chance_with_looting" => LootCondition::RandomChanceWithLooting {
            chance: float(&v["chance"], path)?,
            looting_multiplier: float(&v["looting_multiplier"], path)?,
        },
        "killed_by_player" => LootCondition::KilledByPlayer,
        "killed_by_player_or_pets" => LootCondition::KilledByPlayerOrPets,
        "match_tool" => LootCondition::MatchTool {
            item: v
                .get("item")
                .map(|v| text(v, path).map(str::to_owned))
                .transpose()?,
            enchantments: array(v.get("enchantments"), path, |e, at| {
                let name = text(&e["enchantment"], at)?.to_owned();
                let level = |key: &str, default| -> Result<u8> {
                    e["levels"].get(key).map_or(Ok(default), |v| {
                        uint(v, at)?
                            .try_into()
                            .map_err(|_| ParseError::new(at, "enchantment level outside u8"))
                    })
                };
                Ok(EnchantmentRequirement {
                    id: enchantment_id(&name),
                    enchantment: name,
                    min_level: level("range_min", 1)?,
                    max_level: level("range_max", u8::MAX)?,
                })
            })?,
        },
        "table_bonus" => {
            let enchantment = text(&v["enchantment"], path)?.to_owned();
            LootCondition::TableBonus {
                id: enchantment_id(&enchantment),
                enchantment,
                chances: array(v.get("chances"), path, float)?,
            }
        }
        "block_state_property" => {
            let properties = v["properties"]
                .as_object()
                .ok_or_else(|| ParseError::new(path, "properties must be an object"))?
                .iter()
                .map(|(k, value)| {
                    let value = match value {
                        Value::Bool(b) => LootStateValue::Int(i32::from(*b)),
                        Value::Number(n) => LootStateValue::Int(
                            n.as_i64()
                                .and_then(|n| n.try_into().ok())
                                .ok_or_else(|| ParseError::new(path, "state value outside i32"))?,
                        ),
                        Value::String(s) => LootStateValue::Str(s.clone()),
                        _ => {
                            return Err(ParseError::new(
                                path,
                                "state value must be integer, bool or string",
                            ));
                        }
                    };
                    Ok((k.clone(), value))
                })
                .collect::<Result<Vec<_>>>()?;
            LootCondition::BlockStateProperty { properties }
        }
        "inverted" => LootCondition::Inverted(Box::new(condition(&v["term"], path)?)),
        "entity_properties"
            if v["entity"] == "this"
                && v["properties"]
                    .as_object()
                    .is_some_and(|fields| fields.len() == 1)
                && v["properties"]["on_fire"].is_boolean() =>
        {
            LootCondition::ThisOnFire(
                v["properties"]["on_fire"]
                    .as_bool()
                    .expect("boolean fire state"),
            )
        }
        "is_baby" => LootCondition::IsBaby,
        "passenger_of_entity" => {
            let entity_type = text(&v["entity_type"], path)?;
            LootCondition::PassengerOfEntity {
                entity_type: if entity_type.contains(':') {
                    entity_type.to_owned()
                } else {
                    format!("minecraft:{entity_type}")
                },
            }
        }
        "random_difficulty_chance" => {
            let default = float_or(v, "default_chance", 0.0, path)?;
            let mut chances = [default; 4];
            for (chance, key) in chances
                .iter_mut()
                .zip(["peaceful", "easy", "normal", "hard"])
            {
                *chance = float_or(v, key, default, path)?;
            }
            LootCondition::RandomDifficultyChance { chances }
        }
        "random_regional_difficulty_chance" => LootCondition::RandomRegionalDifficultyChance {
            max_chance: float_or(v, "max_chance", 0.0, path)?,
        },
        _ => LootCondition::Other {
            name: name.to_owned(),
            json: v.clone(),
        },
    })
}

fn function(v: &Value, path: &str) -> Result<LootFunction> {
    let name = text(&v["function"], path)?;
    let kind = match bare(name) {
        "set_count" => LootFunctionKind::SetCount {
            count: number(&v["count"], path)?,
            add: v.get("add").map_or(Ok(false), |v| {
                v.as_bool()
                    .ok_or_else(|| ParseError::new(path, "add must be boolean"))
            })?,
        },
        "set_data" => LootFunctionKind::SetData {
            data: number(&v["data"], path)?,
        },
        "looting_enchant" => LootFunctionKind::LootingEnchant {
            count: number(&v["count"], path)?,
            limit: v.get("limit").map_or(Ok(0), |v| uint(v, path))?,
        },
        "apply_bonus" => {
            let enchantment = text(&v["enchantment"], path)?.to_owned();
            let parameters = &v["parameters"];
            let formula = match text(&v["formula"], path)? {
                "ore_drops" => BonusFormula::OreDrops,
                "uniform_bonus_count" => BonusFormula::UniformBonusCount {
                    bonus_multiplier: float(&parameters["bonusMultiplier"], path)?,
                },
                "binomial_with_bonus_count" => BonusFormula::BinomialWithBonusCount {
                    extra: uint(&parameters["extra"], path)?,
                    probability: float(&parameters["probability"], path)?,
                },
                other => {
                    return Err(ParseError::new(
                        path,
                        format!("unknown apply_bonus formula {other}"),
                    ));
                }
            };
            LootFunctionKind::ApplyBonus {
                id: enchantment_id(&enchantment),
                enchantment,
                formula,
            }
        }
        "limit_count" => LootFunctionKind::LimitCount {
            min: v["limit"].get("min").map(|v| float(v, path)).transpose()?,
            max: v["limit"].get("max").map(|v| float(v, path)).transpose()?,
        },
        "explosion_decay" => LootFunctionKind::ExplosionDecay,
        "furnace_smelt" => LootFunctionKind::FurnaceSmelt,
        "set_data_from_color_index" => LootFunctionKind::SetDataFromColorIndex,
        "enchant_random_gear" => LootFunctionKind::EnchantRandomGear {
            chance: float_or(v, "chance", 0.0, path)?,
        },
        _ => LootFunctionKind::Other {
            name: name.to_owned(),
            json: v.clone(),
        },
    };
    Ok(LootFunction {
        conditions: array(v.get("conditions"), path, condition)?,
        kind,
    })
}

fn entry(v: &Value, path: &str) -> Result<LootEntry> {
    let kind = match v
        .get("type")
        .map(|v| text(v, path))
        .transpose()?
        .unwrap_or("item")
    {
        "item" => LootEntryKind::Item(text(&v["name"], path)?.to_owned()),
        "loot_table" => {
            let name = text(&v["name"], path)?;
            LootEntryKind::Table(
                name.strip_prefix("loot_tables/")
                    .unwrap_or(name)
                    .trim_end_matches(".json")
                    .to_owned(),
            )
        }
        "empty" => LootEntryKind::Empty,
        "alternatives" => LootEntryKind::Alternatives(array(v.get("children"), path, entry)?),
        other => return Err(ParseError::new(path, format!("unknown entry type {other}"))),
    };
    Ok(LootEntry {
        kind,
        weight: v.get("weight").map_or(Ok(1), |v| uint(v, path))?,
        quality: v.get("quality").map_or(Ok(0), |v| {
            v.as_i64()
                .and_then(|n| n.try_into().ok())
                .ok_or_else(|| ParseError::new(path, "quality outside i32"))
        })?,
        conditions: array(v.get("conditions"), path, condition)?,
        functions: array(v.get("functions"), path, function)?,
        nested_pools: array(v.get("pools"), &format!("{path}/pools"), pool)?,
    })
}

fn pool(v: &Value, path: &str) -> Result<LootPool> {
    if !v.is_object() {
        return Err(ParseError::new(path, "pool must be an object"));
    }
    Ok(LootPool {
        rolls: v
            .get("rolls")
            .map_or(Ok(LootNumber::Constant(1.0)), |v| number(v, path))?,
        bonus_rolls: v.get("bonus_rolls").map_or(Ok(0.0), |v| float(v, path))?,
        conditions: array(v.get("conditions"), path, condition)?,
        entries: array(v.get("entries"), path, entry)?,
        tiers: v
            .get("tiers")
            .map(|tiers| -> Result<LootTiers> {
                Ok(LootTiers {
                    initial_range: int_or(tiers, "initial_range", 0, path)?,
                    bonus_rolls: int_or(tiers, "bonus_rolls", 0, path)?,
                    bonus_chance: float_or(tiers, "bonus_chance", 0.0, path)?,
                })
            })
            .transpose()?,
    })
}

/// Decode a raw Bedrock loot table. All source JSON is retained, including
/// unknown conditions/functions. Unsupported behaviour fails explicitly when
/// evaluated; decoding never silently drops it.
pub fn decode_table(name: &str, source: &str, value: &Value) -> Result<LootTable> {
    if !value.is_object() {
        return Err(ParseError::new(name, "table must be an object"));
    }
    Ok(LootTable {
        name: name.to_owned(),
        source: source.to_owned(),
        source_note: value
            .get("//source")
            .and_then(Value::as_str)
            .map(str::to_owned),
        pools: array(value.get("pools"), name, pool)?,
        canonical: value.clone(),
    })
}
