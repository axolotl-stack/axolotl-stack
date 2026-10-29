//! Packet-authoritative crafting-grid validation (items research §2.2).
//!
//! Compile after the item registry and CraftingCatalog freeze. Matching uses
//! only numeric item identities and bounded stack storage; this module never
//! mutates containers, allocates stack network IDs, or implements multi recipes.
//! The request engine owns consumption, output placement and transaction order.

use std::collections::{HashMap, HashSet};

use jolyne::valentine::{
    CerealizerNetworkItemInstanceDescriptorSerializedData as WireResult,
    CerealizerRecipeIngredientSerializedData as WireIngredient,
};
use unastar_rules::molang::eval::{self, MolangContext, Value};
use unastar_rules::molang::{self, BuiltinQuery, Expr, QueryContext, QueryId, QueryRegistry};

use super::CraftingCatalog;
use crate::item::ItemStack;
use crate::registry::ItemRegistry;
use crate::registry::item::ItemEntry;

const MAX_CELLS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSize {
    TwoByTwo,
    ThreeByThree,
}

impl GridSize {
    fn width(self) -> usize {
        match self {
            Self::TwoByTwo => 2,
            Self::ThreeByThree => 3,
        }
    }

    /// UI container slot for a row-major grid index.
    pub fn ui_slot(self, index: usize) -> Option<u8> {
        let (first, len) = match self {
            Self::TwoByTwo => (28, 4),
            Self::ThreeByThree => (32, 9),
        };
        (index < len).then(|| first + u8::try_from(index).expect("bounded grid index"))
    }
}

/// Row-major, fixed-size references into the request's staged UI inventory.
#[derive(Debug, Clone, Copy)]
pub enum CraftingGrid<'a> {
    Inventory([&'a ItemStack; 4]),
    Workbench([&'a ItemStack; 9]),
}

impl CraftingGrid<'_> {
    pub fn size(&self) -> GridSize {
        match self {
            Self::Inventory(_) => GridSize::TwoByTwo,
            Self::Workbench(_) => GridSize::ThreeByThree,
        }
    }

    fn cells(&self) -> &[&ItemStack] {
        match self {
            Self::Inventory(cells) => cells,
            Self::Workbench(cells) => cells,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("crafting recipe network ID {net_id}: {reason}")]
pub struct CompileError {
    pub net_id: u32,
    pub reason: String,
}

fn compile_error(net_id: u32, reason: impl Into<String>) -> CompileError {
    CompileError {
        net_id,
        reason: reason.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("unknown recipe network ID {0}")]
    UnknownRecipe(u32),
    #[error("recipe network ID {0} requires a non-grid crafting algorithm")]
    UnsupportedRecipe(u32),
    #[error("number of crafts must be nonzero")]
    ZeroCrafts,
    #[error("recipe requires a 3x3 crafting grid")]
    RequiresThreeByThree,
    #[error("crafting grid does not match the advertised recipe")]
    GridMismatch,
    #[error("UI slot {ui_slot} contains {available} items but {required} crafts were requested")]
    InsufficientInput {
        ui_slot: u8,
        available: u8,
        required: u32,
    },
    #[error("crafted output count exceeds u32")]
    OutputCountOverflow,
}

/// A packet-derived stack template and its total count across all crafts.
/// The engine must split the total into actual slots using item stack limits;
/// changing the template or narrowing this count would lose advertised data.
#[derive(Debug, Clone, Copy)]
pub struct CraftOutput<'a> {
    pub template: &'a ItemStack,
    pub count: u32,
}

#[derive(Debug)]
pub struct CraftPlan<'a> {
    templates: &'a [ItemStack],
    num_crafts: u32,
    pub grid_size: GridSize,
    /// Per row-major cell, not per descriptor stack_size. Unused entries in a
    /// 2x2 grid are zero; every occupied matching cell consumes num_crafts.
    pub consumption: [u8; MAX_CELLS],
}

impl CraftPlan<'_> {
    pub fn outputs(&self) -> impl ExactSizeIterator<Item = CraftOutput<'_>> {
        self.templates.iter().map(|template| CraftOutput {
            template,
            count: u32::from(template.count)
                .checked_mul(self.num_crafts)
                .expect("output multiplication validated before constructing plan"),
        })
    }

    pub fn ui_consumption(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.consumption
            .iter()
            .enumerate()
            .filter(|&(_, &count)| count != 0)
            .map(|(index, &count)| {
                (
                    self.grid_size
                        .ui_slot(index)
                        .expect("occupied bounded grid cell"),
                    count,
                )
            })
    }
}

#[derive(Debug)]
enum Ingredient {
    Empty,
    Items {
        network_ids: Box<[i32]>,
        aux: Option<i16>,
    },
}

impl Ingredient {
    fn matches(&self, stack: &ItemStack) -> bool {
        match self {
            Self::Empty => stack.is_empty(),
            Self::Items { network_ids, aux } => {
                !stack.is_empty()
                    && stack
                        .item()
                        .is_some_and(|item| network_ids.binary_search(&item.network_id()).is_ok())
                    && aux.is_none_or(|aux| aux == stack.damage)
            }
        }
    }
}

#[derive(Debug)]
enum Layout {
    Shaped {
        width: usize,
        height: usize,
        symmetry: bool,
    },
    Shapeless {
        len: usize,
    },
}

#[derive(Debug)]
struct Recipe {
    ingredients: [Ingredient; MAX_CELLS],
    layout: Layout,
    outputs: Box<[ItemStack]>,
}

impl Recipe {
    fn requires_3x3(&self) -> bool {
        match self.layout {
            Layout::Shaped { width, height, .. } => width >= 3 || height >= 3,
            Layout::Shapeless { len } => len > 4,
        }
    }

    fn matches(&self, grid: &CraftingGrid<'_>) -> bool {
        let cells = grid.cells();
        let grid_width = grid.size().width();
        match self.layout {
            Layout::Shaped {
                width,
                height,
                symmetry,
            } => {
                if width > grid_width || height > grid_width {
                    return false;
                }
                for offset_y in 0..=grid_width - height {
                    for offset_x in 0..=grid_width - width {
                        for mirrored in [false, true] {
                            if mirrored && !symmetry {
                                continue;
                            }
                            let matches = cells.iter().enumerate().all(|(index, stack)| {
                                let x = index % grid_width;
                                let y = index / grid_width;
                                if x < offset_x
                                    || y < offset_y
                                    || x >= offset_x + width
                                    || y >= offset_y + height
                                {
                                    return stack.is_empty();
                                }
                                let x = x - offset_x;
                                let x = if mirrored { width - 1 - x } else { x };
                                self.ingredients[(y - offset_y) * width + x].matches(stack)
                            });
                            if matches {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            Layout::Shapeless { len } => {
                // BDS removes the FIRST matching remaining ingredient for each
                // occupied grid cell in order. Deliberately no backtracking.
                let mut used = [false; MAX_CELLS];
                let mut matched = 0;
                for stack in cells.iter().filter(|stack| !stack.is_empty()) {
                    let Some(index) = (0..len)
                        .find(|&index| !used[index] && self.ingredients[index].matches(stack))
                    else {
                        return false;
                    };
                    used[index] = true;
                    matched += 1;
                }
                matched == len
            }
        }
    }
}

/// Startup-compiled execution index for exactly the IDs advertised by a catalog.
#[derive(Debug)]
pub struct CraftingExecutor {
    recipes: HashMap<u32, Recipe>,
    non_grid: HashSet<u32>,
}

impl CraftingExecutor {
    pub fn new(catalog: &CraftingCatalog, items: &ItemRegistry) -> Result<Self, CompileError> {
        let packet = catalog.packet();
        let mut executor = Self {
            recipes: HashMap::new(),
            non_grid: HashSet::new(),
        };
        for recipe in &packet.shaped_recipes {
            let id = recipe.net_id.raw_id;
            let width = usize::try_from(recipe.width)
                .map_err(|_| compile_error(id, "negative shaped width"))?;
            let height = usize::try_from(recipe.height)
                .map_err(|_| compile_error(id, "negative shaped height"))?;
            if !(1..=3).contains(&width)
                || !(1..=3).contains(&height)
                || width.checked_mul(height) != Some(recipe.ingredients.len())
            {
                return Err(compile_error(
                    id,
                    "invalid bounded shaped dimensions/ingredient count",
                ));
            }
            executor.insert(
                catalog,
                items,
                id,
                &recipe.ingredients,
                &recipe.results,
                Layout::Shaped {
                    width,
                    height,
                    symmetry: recipe.assume_symmetry,
                },
            )?;
        }
        for recipe in packet
            .shapeless_recipes
            .iter()
            .chain(&packet.user_data_shapeless_recipes)
        {
            let id = recipe.net_id.raw_id;
            if recipe.ingredients.len() > MAX_CELLS {
                return Err(compile_error(
                    id,
                    "shapeless recipe exceeds nine ingredient cells",
                ));
            }
            executor.insert(
                catalog,
                items,
                id,
                &recipe.ingredients,
                &recipe.results,
                Layout::Shapeless {
                    len: recipe.ingredients.len(),
                },
            )?;
        }
        for id in packet
            .multi_recipes
            .iter()
            .map(|r| r.net_id.raw_id)
            .chain(
                packet
                    .smithing_transform_recipes
                    .iter()
                    .map(|r| r.net_id.raw_id),
            )
            .chain(packet.smithing_trim_recipes.iter().map(|r| r.net_id.raw_id))
        {
            executor.check_id(catalog, id)?;
            executor.non_grid.insert(id);
        }
        Ok(executor)
    }

    fn check_id(&self, catalog: &CraftingCatalog, id: u32) -> Result<(), CompileError> {
        if catalog.origin(id).is_none() {
            return Err(compile_error(
                id,
                "advertised network ID has no catalog origin",
            ));
        }
        if self.recipes.contains_key(&id) || self.non_grid.contains(&id) {
            return Err(compile_error(id, "duplicate advertised network ID"));
        }
        Ok(())
    }

    fn insert(
        &mut self,
        catalog: &CraftingCatalog,
        items: &ItemRegistry,
        id: u32,
        ingredients: &[WireIngredient],
        outputs: &[WireResult],
        layout: Layout,
    ) -> Result<(), CompileError> {
        self.check_id(catalog, id)?;
        let mut compiled = std::array::from_fn(|_| Ingredient::Empty);
        for (target, ingredient) in compiled.iter_mut().zip(ingredients) {
            *target = compile_ingredient(id, ingredient, items)?;
        }
        let outputs = outputs
            .iter()
            .map(|output| compile_output(id, output, items))
            .collect::<Result<Box<[_]>, _>>()?;
        self.recipes.insert(
            id,
            Recipe {
                ingredients: compiled,
                layout,
                outputs,
            },
        );
        Ok(())
    }

    fn recipe(&self, net_id: u32) -> Result<&Recipe, ValidationError> {
        self.recipes.get(&net_id).ok_or_else(|| {
            if self.non_grid.contains(&net_id) {
                ValidationError::UnsupportedRecipe(net_id)
            } else {
                ValidationError::UnknownRecipe(net_id)
            }
        })
    }

    /// Dimensional requirement only: station tag is NOT a grid-screen check.
    pub fn requires_3x3(&self, net_id: u32) -> Result<bool, ValidationError> {
        self.recipe(net_id).map(Recipe::requires_3x3)
    }

    pub fn validate(
        &self,
        net_id: u32,
        grid: CraftingGrid<'_>,
        num_crafts: u32,
    ) -> Result<CraftPlan<'_>, ValidationError> {
        let recipe = self.recipe(net_id)?;
        if num_crafts == 0 {
            return Err(ValidationError::ZeroCrafts);
        }
        if recipe.requires_3x3() && grid.size() == GridSize::TwoByTwo {
            return Err(ValidationError::RequiresThreeByThree);
        }
        for output in &recipe.outputs {
            u32::from(output.count)
                .checked_mul(num_crafts)
                .ok_or(ValidationError::OutputCountOverflow)?;
        }
        if !recipe.matches(&grid) {
            return Err(ValidationError::GridMismatch);
        }
        let mut consumption = [0; MAX_CELLS];
        for (index, stack) in grid.cells().iter().enumerate() {
            if stack.is_empty() {
                continue;
            }
            if u32::from(stack.count) < num_crafts {
                return Err(ValidationError::InsufficientInput {
                    ui_slot: grid.size().ui_slot(index).expect("bounded grid index"),
                    available: stack.count,
                    required: num_crafts,
                });
            }
            consumption[index] = u8::try_from(num_crafts)
                .expect("craft count is bounded by the occupied input stack");
        }
        Ok(CraftPlan {
            templates: &recipe.outputs,
            num_crafts,
            grid_size: grid.size(),
            consumption,
        })
    }
}

fn compile_ingredient(
    id: u32,
    wire: &WireIngredient,
    items: &ItemRegistry,
) -> Result<Ingredient, CompileError> {
    if wire.descriptor.is_empty() {
        return Ok(Ingredient::Empty);
    }
    if wire.descriptor.len() != 1 {
        return Err(compile_error(
            id,
            "multiple descriptor keys require measured combination semantics",
        ));
    }
    let descriptor = &wire.descriptor[0];
    let mut network_ids = match descriptor.key.as_ref() {
        "name" => vec![
            items
                .get_by_name(descriptor.value.as_ref())
                .ok_or_else(|| {
                    compile_error(id, format!("unknown ingredient {}", descriptor.value))
                })?
                .network_id(),
        ],
        "item_tag" => items
            .iter()
            .filter(|item| item.has_tag(descriptor.value.as_ref()))
            .map(ItemEntry::network_id)
            .collect(),
        "tags" => compile_tag_expression(id, descriptor.value.as_ref(), items)?,
        key => {
            return Err(compile_error(
                id,
                format!("unsupported ingredient descriptor {key}"),
            ));
        }
    };
    network_ids.sort_unstable();
    network_ids.dedup();
    let aux = if wire.aux_value == 32767 {
        None
    } else {
        Some(
            i16::try_from(wire.aux_value)
                .map_err(|_| compile_error(id, "ingredient aux does not fit i16"))?,
        )
    };
    // stack_size is deliberately not part of matching or consumption. The
    // packet has already expanded counted shapeless inputs into separate cells.
    Ok(Ingredient::Items {
        network_ids: network_ids.into_boxed_slice(),
        aux,
    })
}

fn compile_output(
    id: u32,
    wire: &WireResult,
    items: &ItemRegistry,
) -> Result<ItemStack, CompileError> {
    let item = items
        .item_type_by_network_id(wire.id)
        .ok_or_else(|| compile_error(id, format!("unknown output item network ID {}", wire.id)))?;
    let count = u8::try_from(wire.stacksize)
        .map_err(|_| compile_error(id, "advertised output stack count does not fit ItemStack"))?;
    // Catalog writes signed aux as u32. Recover that bit pattern before the
    // checked i16 conversion so negative aux is preserved without narrowing.
    let aux = i32::from_le_bytes(wire.auxvalue.to_le_bytes());
    let damage = i16::try_from(aux)
        .map_err(|_| compile_error(id, "advertised output aux does not fit ItemStack"))?;
    let mut stack = ItemStack::new(item, count)
        .with_damage(damage)
        .with_runtime_id(wire.block_runtime_id);
    // Preserve the exact frozen buffer, including a noncanonical empty buffer.
    // with_nbt intentionally normalizes defaults and is unsuitable here.
    stack.nbt = Some(wire.user_data_buffer.clone());
    Ok(stack)
}

fn compile_tag_expression(
    id: u32,
    source: &str,
    items: &ItemRegistry,
) -> Result<Vec<i32>, CompileError> {
    let mut expression = molang::parse(source)
        .map_err(|reason| compile_error(id, format!("ingredient Molang: {reason}")))?;
    // Only pure item-tag expressions can be reduced to frozen numeric IDs.
    // Fail startup on unavailable context, never silently evaluate it as zero.
    let mut unsupported = None;
    expression.walk(&mut |node| match node {
        Expr::Query(query, args) => {
            if !matches!(query.name.as_ref(), "any_tag" | "all_tags") {
                unsupported = Some(format!(
                    "ingredient query.{} needs an item-context binding",
                    query.name
                ));
            } else if !args.iter().all(|arg| matches!(arg, Expr::Str(_))) {
                unsupported = Some("ingredient tag queries require string tag arguments".into());
            }
        }
        Expr::Math(function, _) if function.is_random() => {
            unsupported = Some(
                "random ingredient expressions cannot be frozen to numeric item predicates".into(),
            );
        }
        Expr::This
        | Expr::Var(..)
        | Expr::Assign(..)
        | Expr::Arrow(..)
        | Expr::Loop(..)
        | Expr::Break
        | Expr::Continue => {
            unsupported =
                Some("ingredient expression requires unavailable mutable/entity context".into());
        }
        _ => {}
    });
    if let Some(reason) = unsupported {
        return Err(compile_error(id, reason));
    }
    expression
        .bind(QueryRegistry::vanilla(), QueryContext::Runtime)
        .map_err(|reason| compile_error(id, reason))?;
    items
        .iter()
        .filter_map(
            |item| match eval::evaluate(&expression, &mut ItemTagContext(item)) {
                Ok(value) if value.truthy() => Some(Ok(item.network_id())),
                Ok(_) => None,
                Err(error) => Some(Err(compile_error(
                    id,
                    format!("ingredient Molang: {error}"),
                ))),
            },
        )
        .collect()
}

/// Used only at startup, after the AST rejects every non-item binding.
struct ItemTagContext<'a>(&'a ItemEntry);

impl MolangContext for ItemTagContext<'_> {
    fn query(&mut self, id: QueryId, args: &[Value]) -> Option<Value> {
        let has_tag = |value: &Value| match value {
            Value::Str(tag) => self.0.has_tag(tag),
            _ => unreachable!("tag query arguments validated as string literals"),
        };
        let result = match BuiltinQuery::from_id(id) {
            Some(BuiltinQuery::AllTags) => args.iter().all(has_tag),
            Some(BuiltinQuery::AnyTag) => args.iter().any(has_tag),
            _ => unreachable!("ingredient query set validated before evaluation"),
        };
        Some(Value::Num(if result { 1.0 } else { 0.0 }))
    }

    fn variable(&self, _: &str) -> Option<Value> {
        unreachable!("item predicate has no variables")
    }
    fn set_variable(&mut self, _: &str, _: Value) {
        unreachable!("item predicate has no assignments")
    }
    fn context(&self, _: &str) -> Option<Value> {
        unreachable!("item predicate has no context variables")
    }
    fn this(&self) -> Value {
        unreachable!("item predicate has no entity")
    }
    fn enter(&mut self, _: u64) -> bool {
        unreachable!("item predicate has no entity arrow")
    }
    fn leave(&mut self) {
        unreachable!("item predicate has no entity arrow")
    }
    fn random(&mut self) -> f32 {
        unreachable!("item predicate has no random operations")
    }
}

#[cfg(test)]
mod tests {
    use super::super::RecipeOrigin;
    use super::*;
    use crate::registry::item::test_support::registry;
    use jolyne::valentine::{
        CerealizerRecipeIngredientSerializedDataDescriptorItem as Descriptor, CraftingDataPacket,
        ShapedRecipePayload, ShapelessRecipePayload,
        TypedServerNetIdstructRecipeNetIdTag as RecipeNetId,
    };
    use std::sync::Arc;

    fn items() -> ItemRegistry {
        registry(&[("test:a", 7, 64), ("test:b", -9, 64), ("test:out", 30, 64)])
    }

    fn input(name: &str, aux: i32, count: i32) -> WireIngredient {
        WireIngredient {
            descriptor: vec![Descriptor {
                key: "name".into(),
                value: name.into(),
            }],
            aux_value: aux,
            stack_size: count,
        }
    }

    fn output(count: u16) -> WireResult {
        WireResult {
            id: 30,
            auxvalue: 0,
            stacksize: count,
            block_runtime_id: 0,
            user_data_buffer: bytes::Bytes::from_static(&[0; 10]),
        }
    }

    fn catalog(packet: CraftingDataPacket, origins: Vec<RecipeOrigin>) -> CraftingCatalog {
        CraftingCatalog {
            packet: Arc::new(packet),
            origins,
        }
    }

    fn shaped(symmetry: bool) -> CraftingCatalog {
        catalog(
            CraftingDataPacket {
                shaped_recipes: vec![ShapedRecipePayload {
                    width: 2,
                    height: 1,
                    ingredients: vec![input("test:a", 32767, 63), input("test:b", 0, 1)],
                    results: vec![output(2)],
                    assume_symmetry: symmetry,
                    net_id: RecipeNetId { raw_id: 1 },
                    ..Default::default()
                }],
                ..Default::default()
            },
            vec![RecipeOrigin::Pack(0)],
        )
    }

    fn shapeless(ingredients: Vec<WireIngredient>, user_data: bool) -> CraftingCatalog {
        let recipe = ShapelessRecipePayload {
            ingredients,
            results: vec![output(2)],
            tag: "furnace".into(),
            net_id: RecipeNetId { raw_id: 1 },
            ..Default::default()
        };
        let mut packet = CraftingDataPacket::default();
        let origin = if user_data {
            packet.user_data_shapeless_recipes.push(recipe);
            RecipeOrigin::UserData(0)
        } else {
            packet.shapeless_recipes.push(recipe);
            RecipeOrigin::Pack(0)
        };
        catalog(packet, vec![origin])
    }

    #[test]
    fn shaped_offsets_mirroring_and_outside_cells_are_exact() {
        let items = items();
        let executor = CraftingExecutor::new(&shaped(true), &items).unwrap();
        let a = items.stack("test:a", 5).unwrap().with_damage(12);
        let b = items.stack("test:b", 5).unwrap();
        let empty = ItemStack::EMPTY;
        for y in 0..3 {
            for x in 0..2 {
                for mirrored in [false, true] {
                    let mut grid = [&empty; 9];
                    grid[y * 3 + x] = if mirrored { &b } else { &a };
                    grid[y * 3 + x + 1] = if mirrored { &a } else { &b };
                    assert!(
                        executor
                            .validate(1, CraftingGrid::Workbench(grid), 1)
                            .is_ok()
                    );
                    let outside = (0..9).find(|&i| grid[i].is_empty()).unwrap();
                    grid[outside] = &a;
                    assert_eq!(
                        executor
                            .validate(1, CraftingGrid::Workbench(grid), 1)
                            .unwrap_err(),
                        ValidationError::GridMismatch
                    );
                }
            }
        }
        let asymmetric = CraftingExecutor::new(&shaped(false), &items).unwrap();
        assert_eq!(
            asymmetric
                .validate(1, CraftingGrid::Inventory([&b, &a, &empty, &empty]), 1)
                .unwrap_err(),
            ValidationError::GridMismatch
        );
        assert!(
            asymmetric
                .validate(1, CraftingGrid::Inventory([&empty, &empty, &a, &b]), 1)
                .is_ok()
        );
        let wrong_aux = b.clone().with_damage(1);
        assert_eq!(
            executor
                .validate(
                    1,
                    CraftingGrid::Inventory([&a, &wrong_aux, &empty, &empty]),
                    1
                )
                .unwrap_err(),
            ValidationError::GridMismatch
        );
    }

    #[test]
    fn shaped_pattern_holes_must_be_empty() {
        let items = items();
        let mut catalog = shaped(false);
        let recipe = &mut Arc::get_mut(&mut catalog.packet).unwrap().shaped_recipes[0];
        recipe.width = 3;
        recipe.ingredients.insert(
            1,
            WireIngredient {
                descriptor: vec![],
                aux_value: 32767,
                stack_size: 0,
            },
        );
        let executor = CraftingExecutor::new(&catalog, &items).unwrap();
        let a = items.stack("test:a", 1).unwrap();
        let b = items.stack("test:b", 1).unwrap();
        let empty = ItemStack::EMPTY;
        assert!(
            executor
                .validate(
                    1,
                    CraftingGrid::Workbench([
                        &a, &empty, &b, &empty, &empty, &empty, &empty, &empty, &empty
                    ]),
                    1
                )
                .is_ok()
        );
        assert_eq!(
            executor
                .validate(
                    1,
                    CraftingGrid::Workbench([
                        &a, &a, &b, &empty, &empty, &empty, &empty, &empty, &empty
                    ]),
                    1
                )
                .unwrap_err(),
            ValidationError::GridMismatch
        );
        assert_eq!(
            executor
                .validate(1, CraftingGrid::Inventory([&a, &b, &empty, &empty]), 1)
                .unwrap_err(),
            ValidationError::RequiresThreeByThree
        );
    }

    #[test]
    fn shapeless_uses_greedy_first_match_without_backtracking() {
        let mut items = items();
        let a_id = items.get_by_name("test:a").unwrap().id;
        let b_id = items.get_by_name("test:b").unwrap().id;
        for id in [a_id, b_id] {
            items
                .update(id, |_, components| components.tags.push("test:both".into()))
                .unwrap();
        }
        let broad = WireIngredient {
            descriptor: vec![Descriptor {
                key: "item_tag".into(),
                value: "test:both".into(),
            }],
            aux_value: 32767,
            stack_size: 64,
        };
        let catalog = shapeless(vec![broad, input("test:a", 32767, 1)], false);
        let executor = CraftingExecutor::new(&catalog, &items).unwrap();
        let a = items.stack("test:a", 3).unwrap();
        let b = items.stack("test:b", 3).unwrap();
        let empty = ItemStack::EMPTY;
        assert_eq!(
            executor
                .validate(1, CraftingGrid::Inventory([&a, &b, &empty, &empty]), 1)
                .unwrap_err(),
            ValidationError::GridMismatch
        );
        assert!(
            executor
                .validate(1, CraftingGrid::Inventory([&b, &a, &empty, &empty]), 1)
                .is_ok()
        );
        assert_eq!(
            executor
                .validate(1, CraftingGrid::Inventory([&b, &empty, &empty, &empty]), 1)
                .unwrap_err(),
            ValidationError::GridMismatch
        );
        assert_eq!(
            executor
                .validate(1, CraftingGrid::Inventory([&b, &a, &a, &empty]), 1)
                .unwrap_err(),
            ValidationError::GridMismatch
        );
    }

    #[test]
    fn num_crafts_consumes_each_cell_not_advertised_ingredient_counts() {
        let items = items();
        let executor = CraftingExecutor::new(&shaped(false), &items).unwrap();
        let a = items.stack("test:a", 3).unwrap();
        let b = items.stack("test:b", 2).unwrap();
        let empty = ItemStack::EMPTY;
        let grid = CraftingGrid::Inventory([&empty, &empty, &a, &b]);
        let plan = executor.validate(1, grid, 2).unwrap();
        assert_eq!(plan.consumption, [0, 0, 2, 2, 0, 0, 0, 0, 0]);
        assert_eq!(
            plan.ui_consumption().collect::<Vec<_>>(),
            [(30, 2), (31, 2)]
        );
        assert_eq!(plan.outputs().next().unwrap().count, 4);
        assert_eq!(
            executor.validate(1, grid, 3).unwrap_err(),
            ValidationError::InsufficientInput {
                ui_slot: 31,
                available: 2,
                required: 3
            }
        );
        assert_eq!(
            executor.validate(1, grid, 0).unwrap_err(),
            ValidationError::ZeroCrafts
        );
        assert_eq!(
            executor.validate(1, grid, u32::MAX).unwrap_err(),
            ValidationError::OutputCountOverflow
        );
        assert_eq!(
            executor.validate(1, grid, 256).unwrap_err(),
            ValidationError::InsufficientInput {
                ui_slot: 30,
                available: 3,
                required: 256
            }
        );
    }

    #[test]
    fn shapeless_cell_count_alone_drives_three_by_three_requirement() {
        let items = items();
        let catalog = shapeless((0..5).map(|_| input("test:a", 32767, 1)).collect(), false);
        let executor = CraftingExecutor::new(&catalog, &items).unwrap();
        assert!(executor.requires_3x3(1).unwrap());
        let a = items.stack("test:a", 2).unwrap();
        let empty = ItemStack::EMPTY;
        assert_eq!(
            executor
                .validate(1, CraftingGrid::Inventory([&a; 4]), 1)
                .unwrap_err(),
            ValidationError::RequiresThreeByThree
        );
        let plan = executor
            .validate(
                1,
                CraftingGrid::Workbench([&a, &a, &a, &a, &a, &empty, &empty, &empty, &empty]),
                2,
            )
            .unwrap();
        assert_eq!(
            plan.ui_consumption().collect::<Vec<_>>(),
            [(32, 2), (33, 2), (34, 2), (35, 2), (36, 2)]
        );
        let small =
            CraftingExecutor::new(&shapeless(vec![input("test:a", 32767, 99)], false), &items)
                .unwrap();
        assert!(!small.requires_3x3(1).unwrap());
        // The advertised furnace tag does not restrict matching on this screen.
        assert!(
            small
                .validate(1, CraftingGrid::Inventory([&a, &empty, &empty, &empty]), 1)
                .is_ok()
        );
    }

    #[test]
    fn output_templates_preserve_packet_metadata_and_user_data_recipe_ids() {
        let items = items();
        let mut catalog = shapeless(vec![input("test:a", 32767, 1)], true);
        let result = &mut Arc::get_mut(&mut catalog.packet)
            .unwrap()
            .user_data_shapeless_recipes[0]
            .results[0];
        result.auxvalue = u32::from_le_bytes((-7_i32).to_le_bytes());
        result.stacksize = 3;
        result.block_runtime_id = 9876;
        result.user_data_buffer = bytes::Bytes::from_static(&[255, 255, 1, 10, 42, 0, 9, 8, 7]);
        let expected = result.clone();
        let executor = CraftingExecutor::new(&catalog, &items).unwrap();
        let a = items.stack("test:a", 4).unwrap();
        let empty = ItemStack::EMPTY;
        let plan = executor
            .validate(1, CraftingGrid::Inventory([&a, &empty, &empty, &empty]), 4)
            .unwrap();
        let result = plan.outputs().next().unwrap();
        assert_eq!(result.count, 12);
        assert_eq!(result.template.count, 3);
        assert_eq!(result.template.item().unwrap().network_id(), expected.id);
        assert_eq!(result.template.damage, -7);
        assert_eq!(result.template.runtime_id, expected.block_runtime_id);
        assert_eq!(result.template.user_data(), expected.user_data_buffer);
        assert_eq!(result.template.net_id, 0);
        assert_eq!(
            executor
                .validate(2, CraftingGrid::Inventory([&a; 4]), 1)
                .unwrap_err(),
            ValidationError::UnknownRecipe(2)
        );
    }

    #[test]
    fn tag_expressions_use_shared_case_insensitive_parser_and_freeze_numeric_sets() {
        let mut items = items();
        let a_id = items.get_by_name("test:a").unwrap().id;
        items
            .update(a_id, |_, components| {
                components.tags.push("test:allowed".into())
            })
            .unwrap();
        let ingredient = WireIngredient {
            descriptor: vec![Descriptor {
                key: "tags".into(),
                value: "Q.Any_Tag('test:allowed') && !query.all_tags('test:excluded')".into(),
            }],
            aux_value: 32767,
            stack_size: 1,
        };
        let catalog = shapeless(vec![ingredient], false);
        let executor = CraftingExecutor::new(&catalog, &items).unwrap();
        let a = items.stack("test:a", 1).unwrap();
        let b = items.stack("test:b", 1).unwrap();
        let empty = ItemStack::EMPTY;
        assert!(
            executor
                .validate(1, CraftingGrid::Inventory([&a, &empty, &empty, &empty]), 1)
                .is_ok()
        );
        assert_eq!(
            executor
                .validate(1, CraftingGrid::Inventory([&b, &empty, &empty, &empty]), 1)
                .unwrap_err(),
            ValidationError::GridMismatch
        );
        let mut unsupported = catalog;
        Arc::get_mut(&mut unsupported.packet)
            .unwrap()
            .shapeless_recipes[0]
            .ingredients[0]
            .descriptor[0]
            .value = "query.remaining_durability > 0".into();
        assert!(
            CraftingExecutor::new(&unsupported, &items)
                .unwrap_err()
                .reason
                .contains("item-context binding")
        );
    }

    #[test]
    fn complete_current_catalog_compiles_from_its_advertised_packet() {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let blocks = crate::registry::BlockRegistry::vanilla();
        let recipes = unastar_data::recipes::RecipeRegistry::vanilla().unwrap();
        let catalog = CraftingCatalog::new(&recipes, &items, blocks).unwrap();
        let executor = CraftingExecutor::new(&catalog, &items).unwrap();
        let packet = catalog.packet();
        assert_eq!(
            executor.recipes.len(),
            packet.shaped_recipes.len()
                + packet.shapeless_recipes.len()
                + packet.user_data_shapeless_recipes.len()
        );
        for (id, results) in packet
            .shaped_recipes
            .iter()
            .map(|recipe| (recipe.net_id.raw_id, &recipe.results))
            .chain(
                packet
                    .shapeless_recipes
                    .iter()
                    .chain(&packet.user_data_shapeless_recipes)
                    .map(|recipe| (recipe.net_id.raw_id, &recipe.results)),
            )
        {
            let compiled = &executor.recipes[&id];
            assert_eq!(compiled.outputs.len(), results.len(), "recipe {id}");
            for (template, wire) in compiled.outputs.iter().zip(results) {
                assert_eq!(
                    template.item().unwrap().network_id(),
                    wire.id,
                    "recipe {id}"
                );
                assert_eq!(u16::from(template.count), wire.stacksize, "recipe {id}");
                assert_eq!(
                    i32::from(template.damage),
                    i32::from_le_bytes(wire.auxvalue.to_le_bytes()),
                    "recipe {id}"
                );
                assert_eq!(template.runtime_id, wire.block_runtime_id, "recipe {id}");
                assert_eq!(template.user_data(), wire.user_data_buffer, "recipe {id}");
                assert_eq!(template.net_id, 0, "recipe {id}");
            }
        }
        for id in packet.multi_recipes.iter().map(|r| r.net_id.raw_id) {
            assert_eq!(
                executor.requires_3x3(id).unwrap_err(),
                ValidationError::UnsupportedRecipe(id)
            );
        }
    }
}
