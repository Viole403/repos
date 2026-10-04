//! One cashier shift. A closed shift is immutable history.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "registers")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// 'Open' or 'Closed'.
    pub status: String,
    pub opened_at: chrono::NaiveDateTime,
    pub closed_at: Option<chrono::NaiveDateTime>,
    pub opening_balance: Decimal,
    /// Per-method opening float as JSON. Nullable: an old row predates the field.
    pub opening_details: Option<String>,
    /// What the cashier counted at close. Set only then.
    pub closing_balance: Option<Decimal>,
    /// Derived snapshot at close, so the report cannot drift as later data lands.
    pub expected_balance: Option<Decimal>,
    pub note: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::entities::auth::users::Entity",
        from = "Column::UserId",
        to = "crate::entities::auth::users::Column::Id"
    )]
    User,
}

impl Related<crate::entities::auth::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
