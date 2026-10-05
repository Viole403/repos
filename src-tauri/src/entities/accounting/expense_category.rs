use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Same shape as `income_category` against its own table. The two are separate
/// rather than one table with a `direction` column: an expense category is a list of
/// what the shop spends on and an income category is a list of what it earns outside
/// trading, and merging them would put "Rental income" one rename away from appearing
/// on the expense form.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "expense_categories")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub del_status: String,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::expense::Entity")]
    Expense,
}

impl Related<super::expense::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Expense.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
