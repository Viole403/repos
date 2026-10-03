//! Schema migrations.
//!
//! Mirrors the reference product's role/permission model (spatie-style pivots)
//! so the authorization semantics carry over, plus the master-data tables the
//! catalog and POS screens depend on.
//!
//! Money is `Decimal` (three decimal places) and quantities are `Decimal` too —
//! the reference casts both to `decimal:3`, and fractional quantities are real.

use sea_orm_migration::prelude::*;

// Schema builder types re-exported by sea-orm-migration's prelude.
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};

/// The migration registry. `db::init` runs this before the window opens.
#[derive(Debug)]
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(Migrations::AuthAndRoles),
            Box::new(Migrations::MasterData),
            Box::new(Migrations::Items),
        ]
    }
}

#[derive(DeriveIden)]
pub enum Migrations {
    AuthAndRoles,
    MasterData,
    Items,
}

/// Soft-delete marker used across the reference's tables.
const DEL_LIVE: &str = "Live";

/// Timestamp column type.
///
/// `ColumnType::Timestamp` renders as `timestamp_text` and `TimestampWithTimeZone`
/// as `timestamp_with_timezone_text` on SQLite — neither is valid SQLite DDL and the
/// migration fails with `near "(": syntax error`. `custom("timestamp")` is portable
/// across SQLite, Postgres, and MySQL, so all three stay on the same schema.
const TIMESTAMP: &str = "timestamp";

/// Money/quantity precision: 18 total digits, 3 after the point.
///
/// `ColumnType::Decimal(None)` renders as the literal string `real_decimal` on SQLite,
/// which is not valid DDL — the migration fails with `near "(": syntax error`. Passing
/// an explicit precision makes sea-query emit `real(18,3)`, which every backend accepts.
/// The reference casts money to `decimal:3`, so scale 3 is deliberate.
const DECIMAL_PRECISION: u32 = 18;
const DECIMAL_SCALE: u32 = 3;

#[async_trait::async_trait]
impl MigrationName for Migrations {
    fn name(&self) -> &str {
        match self {
            Migrations::AuthAndRoles => "auth_and_roles",
            Migrations::MasterData => "master_data",
            Migrations::Items => "items",
        }
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migrations {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        match self {
            Migrations::AuthAndRoles => auth_and_roles(manager).await?,
            Migrations::MasterData => master_data(manager).await?,
            Migrations::Items => items(manager).await?,
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Reverse order so drops never violate foreign keys.
        match self {
            Migrations::Items => manager
                .drop_table(Table::drop().table(Items::Table).if_exists().to_owned())
                .await?,
            Migrations::MasterData => {
                for t in [
                    ItemCategories::Table.into_iden(),
                    Brands::Table.into_iden(),
                    Units::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
            Migrations::AuthAndRoles => {
                for t in [
                    RolePermissions::Table.into_iden(),
                    UserRoles::Table.into_iden(),
                    Roles::Table.into_iden(),
                    Permissions::Table.into_iden(),
                    Users::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Users, roles, permissions
// ---------------------------------------------------------------------------

async fn auth_and_roles(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Users::Table)
                .if_not_exists()
                .col(ColumnDef::new(Users::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Users::Name).string().not_null())
                .col(ColumnDef::new(Users::Email).string().not_null().unique_key())
                .col(ColumnDef::new(Users::PasswordHash).string().not_null())
                .col(ColumnDef::new(Users::Phone).string().null())
                .col(ColumnDef::new(Users::Role).string().null())
                .col(ColumnDef::new(Users::Photo).string().null())
                .col(ColumnDef::new(Users::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Users::TwoFactorEnabled).boolean().not_null().default(false))
                .col(ColumnDef::new(Users::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Users::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Permissions::Table)
                .if_not_exists()
                .col(ColumnDef::new(Permissions::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Permissions::Name).string().not_null())
                .col(ColumnDef::new(Permissions::GroupName).string().not_null())
                // The reference omits guard_name (single web guard); kept for parity.
                .col(ColumnDef::new(Permissions::GuardName).string().not_null().default("web"))
                .col(ColumnDef::new(Permissions::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Permissions::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Permissions::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                // reference: unique(['name', 'guard_name', 'group_name'])
                .index(&mut Index::create().name("idx_permissions_unique").col(Permissions::Name).col(Permissions::GuardName).col(Permissions::GroupName).unique().to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Roles::Table)
                .if_not_exists()
                .col(ColumnDef::new(Roles::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Roles::Name).string().not_null())
                .col(ColumnDef::new(Roles::GuardName).string().not_null().default("web"))
                // reference enum('role_type', ['Master', 'Other'])
                .col(ColumnDef::new(Roles::RoleType).string().not_null().default("Other"))
                .col(ColumnDef::new(Roles::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Roles::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Roles::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .index(&mut Index::create().name("idx_roles_unique").col(Roles::Name).col(Roles::GuardName).unique().to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(UserRoles::Table)
                .if_not_exists()
                .col(ColumnDef::new(UserRoles::RoleId).integer().not_null())
                .col(ColumnDef::new(UserRoles::UserId).integer().not_null())
                .primary_key(&mut Index::create().name("pk_user_roles").col(UserRoles::RoleId).col(UserRoles::UserId).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_user_roles_role").from(UserRoles::Table, UserRoles::RoleId).to(Roles::Table, Roles::Id).on_delete(ForeignKeyAction::Cascade).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_user_roles_user").from(UserRoles::Table, UserRoles::UserId).to(Users::Table, Users::Id).on_delete(ForeignKeyAction::Cascade).to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(RolePermissions::Table)
                .if_not_exists()
                .col(ColumnDef::new(RolePermissions::PermissionId).integer().not_null())
                .col(ColumnDef::new(RolePermissions::RoleId).integer().not_null())
                .primary_key(&mut Index::create().name("pk_role_permissions").col(RolePermissions::PermissionId).col(RolePermissions::RoleId).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_role_permissions_permission").from(RolePermissions::Table, RolePermissions::PermissionId).to(Permissions::Table, Permissions::Id).on_delete(ForeignKeyAction::Cascade).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_role_permissions_role").from(RolePermissions::Table, RolePermissions::RoleId).to(Roles::Table, Roles::Id).on_delete(ForeignKeyAction::Cascade).to_owned())
                .to_owned(),
        )
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Master data: units, brands, categories
// ---------------------------------------------------------------------------

async fn master_data(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let _ = &manager;
    manager
        .create_table(
            Table::create()
                .table(Units::Table)
                .if_not_exists()
                .col(ColumnDef::new(Units::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Units::UnitName).string().not_null())
                .col(ColumnDef::new(Units::Description).string().null())
                .col(ColumnDef::new(Units::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Units::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Units::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Brands::Table)
                .if_not_exists()
                .col(ColumnDef::new(Brands::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Brands::Name).string().not_null())
                .col(ColumnDef::new(Brands::Description).string().null())
                .col(ColumnDef::new(Brands::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Brands::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Brands::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ItemCategories::Table)
                .if_not_exists()
                .col(ColumnDef::new(ItemCategories::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ItemCategories::Name).string().not_null())
                .col(ColumnDef::new(ItemCategories::Description).string().null())
                .col(ColumnDef::new(ItemCategories::SortId).integer().not_null().default(0))
                .col(ColumnDef::new(ItemCategories::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(ItemCategories::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(ItemCategories::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .index(&mut Index::create().name("idx_item_categories_sort").col(ItemCategories::SortId).to_owned())
                .to_owned(),
        )
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

async fn items(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Items::Table)
                .if_not_exists()
                .col(ColumnDef::new(Items::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Items::Name).string().not_null())
                .col(ColumnDef::new(Items::Code).string().not_null().unique_key())
                .col(ColumnDef::new(Items::AlternativeName).string().null())
                .col(ColumnDef::new(Items::GenericName).string().null())
                .col(ColumnDef::new(Items::Description).string().null())
                .col(ColumnDef::new(Items::CategoryId).integer().null())
                .col(ColumnDef::new(Items::BrandId).integer().null())
                .col(ColumnDef::new(Items::PurchaseUnitId).integer().null())
                .col(ColumnDef::new(Items::SaleUnitId).integer().null())
                .col(ColumnDef::new(Items::ConversionRate).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(1))
                .col(ColumnDef::new(Items::PurchasePrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Items::SalePrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Items::WholeSalePrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Items::AlertQuantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Items::LoyaltyPoint).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Items::Photo).string().null())
                .col(ColumnDef::new(Items::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Items::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Items::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(&mut ForeignKey::create().name("fk_items_category").from(Items::Table, Items::CategoryId).to(ItemCategories::Table, ItemCategories::Id).on_delete(ForeignKeyAction::SetNull).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_items_brand").from(Items::Table, Items::BrandId).to(Brands::Table, Brands::Id).on_delete(ForeignKeyAction::SetNull).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_items_purchase_unit").from(Items::Table, Items::PurchaseUnitId).to(Units::Table, Units::Id).on_delete(ForeignKeyAction::SetNull).to_owned())
                .foreign_key(&mut ForeignKey::create().name("fk_items_sale_unit").from(Items::Table, Items::SaleUnitId).to(Units::Table, Units::Id).on_delete(ForeignKeyAction::SetNull).to_owned())
                .index(&mut Index::create().name("idx_items_name").col(Items::Name).to_owned())
                .to_owned(),
        )
        .await?;

    Ok(())
}

#[derive(Iden)]
enum Users {
    Table,
    Id,
    Name,
    Email,
    PasswordHash,
    Phone,
    Role,
    Photo,
    DelStatus,
    TwoFactorEnabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Permissions {
    Table,
    Id,
    Name,
    GroupName,
    GuardName,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Roles {
    Table,
    Id,
    Name,
    GuardName,
    RoleType,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum UserRoles {
    Table,
    RoleId,
    UserId,
}

#[derive(Iden)]
enum RolePermissions {
    Table,
    PermissionId,
    RoleId,
}

#[derive(Iden)]
enum Units {
    Table,
    Id,
    UnitName,
    Description,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Brands {
    Table,
    Id,
    Name,
    Description,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum ItemCategories {
    Table,
    Id,
    Name,
    Description,
    SortId,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Items {
    Table,
    Id,
    Name,
    Code,
    AlternativeName,
    GenericName,
    Description,
    CategoryId,
    BrandId,
    PurchaseUnitId,
    SaleUnitId,
    ConversionRate,
    PurchasePrice,
    SalePrice,
    WholeSalePrice,
    AlertQuantity,
    LoyaltyPoint,
    Photo,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[cfg(test)]
mod sql_dump {
    use super::*;
    use sea_orm::sea_query::{SqliteQueryBuilder, Table};

    /// Prints each generated CREATE TABLE without executing it, so a malformed
    /// column definition is visible as SQL rather than as an opaque driver error.
    #[test]
    fn dump() {
        let b = SqliteQueryBuilder;
        for stmt in [
            Table::create().table(Units::Table).if_not_exists()
                .col(ColumnDef::new(Units::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Units::UnitName).string().not_null())
                .col(ColumnDef::new(Units::Description).string().null())
                .to_owned(),
            Table::create().table(ItemCategories::Table).if_not_exists()
                .col(ColumnDef::new(ItemCategories::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(ItemCategories::SortId).integer().not_null().default(0))
                .to_owned(),
            Table::create().table(Users::Table).if_not_exists()
                .col(ColumnDef::new(Users::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Users::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        ] {
            println!("SQL>> {}", stmt.to_string(SqliteQueryBuilder));
        }
    }
}
