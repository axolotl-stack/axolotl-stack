use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:economy_trade_table`. Defines this entity's ability to trade with players.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct EconomyTradeTable {
    ///Determines when the mob transforms, if the trades should be converted when the new mob has a economy_trade_table. When the trades are converted, the mob will generate a new trade list with their new trade table, but then it will try to convert any of the same trades over to have the same enchantments and user data. For example, if the original has a Emerald to Enchanted Iron Sword (Sharpness 1), and the new trade also has an Emerald for Enchanted Iron Sword, then the enchantment will be Sharpness 1.
    pub convert_trades_economy: Option<bool>,
    ///How much should the discount be modified by when the player has cured the Zombie Villager. Can be specified as a pair of numbers (low-tier trade discount and high-tier trade discount)
    pub cured_discount: Option<Vec<i32>>,
    ///Name to be displayed while trading with this entity.
    pub display_name: Option<String>,
    ///Used in legacy prices to determine how much should Demand be modified by when the player has the Hero of the Village mob effect.
    pub hero_demand_discount: Option<i32>,
    ///The Maximum the discount can be modified by when the player has cured the Zombie Villager. Can be specified as a pair of numbers (low-tier trade discount and high-tier trade discount)
    pub max_cured_discount: Option<Vec<i32>>,
    ///The Maximum the discount can be modified by when the player has cured a nearby Zombie Villager.
    pub max_nearby_cured_discount: Option<i32>,
    ///How much should the discount be modified by when the player has cured a nearby Zombie Villager.
    pub nearby_cured_discount: Option<i32>,
    ///Used to determine if trading with entity opens the new trade screen.
    pub new_screen: Option<bool>,
    ///Determines if the trades should persist when the mob transforms. This makes it so that the next time the mob is transformed to something with a trade_table or economy_trade_table, then it keeps their trades.
    pub persist_trades: Option<bool>,
    ///Show an in game trade screen when interacting with the mob.
    pub show_trade_screen: Option<bool>,
    ///File path relative to the resource pack root for this entity's trades.
    pub table: Option<String>,
    ///Determines whether the legacy formula is used to determines the trade prices.
    pub use_legacy_price_formula: Option<bool>,
}
impl Default for EconomyTradeTable {
    fn default() -> Self {
        Self {
            convert_trades_economy: Some(false),
            cured_discount: None,
            display_name: None,
            hero_demand_discount: Some(-4i32),
            max_cured_discount: None,
            max_nearby_cured_discount: Some(-200i32),
            nearby_cured_discount: Some(-20i32),
            new_screen: Some(false),
            persist_trades: Some(false),
            show_trade_screen: Some(true),
            table: None,
            use_legacy_price_formula: Some(false),
        }
    }
}
