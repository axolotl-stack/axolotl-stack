use super::*;
use rand::Rng;

const MAX_TABLE_DEPTH: u8 = 16;

/// Facts supplied by the entity/item/block caller. Enchantment names are
/// resolved to protocol ids at decode, so rolls do not parse item NBT or
/// look up enchantment names. Context implementations may cache tool facts.
pub trait LootContext {
    fn tool_item(&self) -> Option<&str>;
    fn enchantment_level(&self, id: i16) -> u8;
    fn block_state_matches(&self, property: &str, expected: &LootStateValue) -> bool;
    fn killed_by_player(&self) -> bool;
    fn looting(&self) -> u8;
    /// None means no THIS actor, not an actor whose fire state is false.
    fn on_fire(&self) -> Option<bool> {
        None
    }
    fn furnace(&self) -> Option<&dyn FurnaceSmelt> {
        None
    }
    /// Palette colour of the THIS actor (`set_data_from_color_index`); `None`
    /// when there is no THIS actor, which leaves the item unchanged.
    fn this_color_index(&self) -> Option<u8> {
        None
    }
    /// The modern item a legacy item name stands for (`minecraft:muttonRaw`
    /// is `minecraft:mutton`), resolved when an entry creates its item.
    fn item_alias(&self, _name: &str) -> Option<&str> {
        None
    }
    /// The flattened item a legacy name selects by data value
    /// (`minecraft:wool` with data 14 is `minecraft:red_wool`).
    fn data_alias(&self, _name: &str, _data: i16) -> Option<&str> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootDrop {
    pub item_id: String,
    pub count: u32,
    pub data: i16,
    /// Present when an implemented function produces item user data.
    pub nbt: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LootError {
    #[error("unsupported loot feature {0}")]
    Unsupported(String),
    #[error("unknown loot table {0}")]
    UnknownTable(String),
    #[error("loot tables nest too deeply")]
    TooDeep,
    #[error("loot context does not supply {0}")]
    MissingContext(&'static str),
}

fn sample_int(number: LootNumber, rng: &mut impl Rng) -> i32 {
    match number {
        LootNumber::Constant(v) => v.floor() as i32,
        LootNumber::Uniform { min, max } => {
            let (min, max) = (min.floor() as i32, max.floor() as i32);
            if max <= min {
                min
            } else {
                rng.gen_range(min..=max)
            }
        }
        LootNumber::Binomial { n, p } => binomial(n, p, rng) as i32,
    }
}

fn binomial(n: u32, p: f32, rng: &mut impl Rng) -> u32 {
    (0..n).filter(|_| rng.r#gen::<f32>() < p).count() as u32
}

fn tool_level(ctx: &dyn LootContext, id: Option<i16>, name: &str) -> Result<u8, LootError> {
    id.map(|id| ctx.enchantment_level(id))
        .ok_or_else(|| LootError::Unsupported(name.to_owned()))
}

fn condition_passes(
    c: &LootCondition,
    ctx: &dyn LootContext,
    rng: &mut impl Rng,
) -> Result<bool, LootError> {
    Ok(match c {
        LootCondition::RandomChance { chance } => rng.r#gen::<f32>() < *chance,
        LootCondition::RandomChanceWithLooting {
            chance,
            looting_multiplier,
        } => rng.r#gen::<f32>() < chance + f32::from(ctx.looting()) * looting_multiplier,
        LootCondition::KilledByPlayer | LootCondition::KilledByPlayerOrPets => {
            ctx.killed_by_player()
        }
        LootCondition::MatchTool { item, enchantments } => {
            let Some(tool) = ctx.tool_item() else {
                return Ok(false);
            };
            if item.as_deref().is_some_and(|item| item != tool) {
                return Ok(false);
            }
            for requirement in enchantments {
                let level = tool_level(ctx, requirement.id, &requirement.enchantment)?;
                if level < requirement.min_level || level > requirement.max_level {
                    return Ok(false);
                }
            }
            true
        }
        LootCondition::TableBonus {
            enchantment,
            id,
            chances,
        } => {
            let level = usize::from(tool_level(ctx, *id, enchantment)?);
            let chance = chances
                .get(level.min(chances.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0.0);
            rng.r#gen::<f32>() < chance
        }
        LootCondition::BlockStateProperty { properties } => properties
            .iter()
            .all(|(p, v)| ctx.block_state_matches(p, v)),
        LootCondition::Inverted(inner) => !condition_passes(inner, ctx, rng)?,
        LootCondition::ThisOnFire(expected) => ctx.on_fire() == Some(*expected),
        LootCondition::Other { name, .. } => return Err(LootError::Unsupported(name.clone())),
    })
}

fn all_pass(
    cs: &[LootCondition],
    ctx: &dyn LootContext,
    rng: &mut impl Rng,
) -> Result<bool, LootError> {
    for c in cs {
        if !condition_passes(c, ctx, rng)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Apply an entry's functions to its item; `authored` is the entry's item
/// name as written (data-selected legacy names re-resolve from it).
fn apply_functions(
    fs: &[LootFunction],
    drop: &mut LootDrop,
    authored: &str,
    ctx: &dyn LootContext,
    rng: &mut impl Rng,
) -> Result<(), LootError> {
    let mut count = i64::from(drop.count);
    for f in fs {
        if !all_pass(&f.conditions, ctx, rng)? {
            continue;
        }
        match &f.kind {
            LootFunctionKind::SetCount { count: n, add } => {
                let n = i64::from(sample_int(*n, rng));
                count = if *add { count + n } else { n };
            }
            LootFunctionKind::SetData { data } => {
                drop.data =
                    sample_int(*data, rng).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
            }
            LootFunctionKind::LootingEnchant { count: n, limit } => {
                count += (0..ctx.looting())
                    .map(|_| i64::from(sample_int(*n, rng)))
                    .sum::<i64>();
                if *limit > 0 {
                    count = count.min(i64::from(*limit));
                }
            }
            LootFunctionKind::ApplyBonus {
                enchantment,
                id,
                formula,
            } => {
                let level = i64::from(tool_level(ctx, *id, enchantment)?);
                if level > 0 {
                    match formula {
                        BonusFormula::OreDrops => {
                            count *= (rng.gen_range(0..level + 2) - 1).max(0) + 1
                        }
                        BonusFormula::UniformBonusCount { bonus_multiplier } => {
                            count += rng.gen_range(0..=(*bonus_multiplier * level as f32) as i64)
                        }
                        BonusFormula::BinomialWithBonusCount { extra, probability } => {
                            count += i64::from(binomial(extra + level as u32, *probability, rng))
                        }
                    }
                } else if let BonusFormula::BinomialWithBonusCount { extra, probability } = formula
                {
                    count += i64::from(binomial(*extra, *probability, rng));
                }
            }
            LootFunctionKind::LimitCount { min, max } => {
                if let Some(min) = min {
                    count = count.max(*min as i64);
                }
                if let Some(max) = max {
                    count = count.min(*max as i64);
                }
            }
            // No explosion caller exists on the parked gameplay path. This is
            // the existing non-explosion identity, not explosion support.
            LootFunctionKind::ExplosionDecay => {}
            LootFunctionKind::FurnaceSmelt => {
                let furnace = ctx
                    .furnace()
                    .ok_or(LootError::MissingContext("furnace_smelt"))?;
                if let Some(result) = furnace.smelt(&drop.item_id, drop.data) {
                    drop.item_id.clear();
                    drop.item_id.push_str(result.identifier);
                    drop.data = result.data;
                }
            }
            LootFunctionKind::SetDataFromColorIndex => {
                if let Some(color) = ctx.this_color_index() {
                    let data = i16::from(color);
                    match ctx.data_alias(authored, data) {
                        Some(item) => {
                            drop.item_id.clear();
                            drop.item_id.push_str(item);
                            drop.data = 0;
                        }
                        None => drop.data = data,
                    }
                }
            }
            LootFunctionKind::Other { name, .. } => {
                return Err(LootError::Unsupported(name.clone()));
            }
        }
    }
    drop.count = count.max(0) as u32;
    Ok(())
}

struct Roller<'a, R> {
    tables: &'a LootTables,
    ctx: &'a dyn LootContext,
    rng: &'a mut R,
    out: Vec<LootDrop>,
    // Eligibility conditions may consume randomness; evaluate once and retain
    // indices. Reuse storage across rolls/pools instead of allocating per roll.
    eligible: Vec<usize>,
}

impl<R: Rng> Roller<'_, R> {
    fn entry(&mut self, entry: &LootEntry, depth: u8) -> Result<(), LootError> {
        match &entry.kind {
            LootEntryKind::Item(item_id) => {
                let resolved = self
                    .ctx
                    .data_alias(item_id, 0)
                    .or_else(|| self.ctx.item_alias(item_id))
                    .unwrap_or(item_id);
                let mut drop = LootDrop {
                    item_id: resolved.to_owned(),
                    count: 1,
                    data: 0,
                    nbt: None,
                };
                apply_functions(&entry.functions, &mut drop, item_id, self.ctx, self.rng)?;
                if drop.count > 0 {
                    self.out.push(drop);
                }
            }
            LootEntryKind::Table(name) => {
                let table = self
                    .tables
                    .get(name)
                    .ok_or_else(|| LootError::UnknownTable(name.clone()))?;
                self.table(table, depth + 1)?;
            }
            LootEntryKind::Empty => {}
            LootEntryKind::Alternatives(children) => {
                for child in children {
                    if all_pass(&child.conditions, self.ctx, self.rng)? {
                        return self.entry(child, depth);
                    }
                }
            }
        }
        if entry.nested_pools_json.is_some() {
            return Err(LootError::Unsupported("entry-level pools".into()));
        }
        Ok(())
    }

    fn pool(&mut self, pool: &LootPool, depth: u8) -> Result<(), LootError> {
        if pool.tiers_json.is_some() {
            return Err(LootError::Unsupported("tiers".into()));
        }
        if !all_pass(&pool.conditions, self.ctx, self.rng)? {
            return Ok(());
        }
        for _ in 0..sample_int(pool.rolls, self.rng).max(0) {
            self.eligible.clear();
            for (i, entry) in pool.entries.iter().enumerate() {
                if entry.weight > 0 && all_pass(&entry.conditions, self.ctx, self.rng)? {
                    self.eligible.push(i);
                }
            }
            let total: u32 = self.eligible.iter().map(|i| pool.entries[*i].weight).sum();
            if total == 0 {
                continue;
            }
            let mut pick = self.rng.gen_range(0..total);
            let mut selected = None;
            for i in &self.eligible {
                let entry = &pool.entries[*i];
                if pick < entry.weight {
                    selected = Some(*i);
                    break;
                }
                pick -= entry.weight;
            }
            if let Some(i) = selected {
                self.entry(&pool.entries[i], depth)?;
            }
        }
        Ok(())
    }

    fn table(&mut self, table: &LootTable, depth: u8) -> Result<(), LootError> {
        if depth > MAX_TABLE_DEPTH {
            return Err(LootError::TooDeep);
        }
        for pool in &table.pools {
            self.pool(pool, depth)?;
        }
        Ok(())
    }
}

/// Roll a decoded table once using the caller's RNG. Nested table references
/// resolve against `tables`; item ids are left for the caller's ItemRegistry.
pub fn roll(
    tables: &LootTables,
    table: &LootTable,
    ctx: &dyn LootContext,
    rng: &mut impl Rng,
) -> Result<Vec<LootDrop>, LootError> {
    let mut roller = Roller {
        tables,
        ctx,
        rng,
        out: Vec::new(),
        eligible: Vec::new(),
    };
    roller.table(table, 0)?;
    Ok(roller.out)
}
