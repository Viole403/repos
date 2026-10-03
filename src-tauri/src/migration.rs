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

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use crate::entities::auth::permissions;

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
            Box::new(Migrations::SalesAndStock),
            Box::new(Migrations::PermissionCatalog),
        ]
    }
}

#[derive(DeriveIden)]
pub enum Migrations {
    AuthAndRoles,
    MasterData,
    Items,
    SalesAndStock,
    PermissionCatalog,
    CustomersAndSuppliers,
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
            Migrations::SalesAndStock => "sales_and_stock",
            Migrations::PermissionCatalog => "permission_catalog",
            Migrations::CustomersAndSuppliers => "customers_and_suppliers",
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
            Migrations::SalesAndStock => sales_and_stock(manager).await?,
            Migrations::PermissionCatalog => permission_catalog(manager).await?,
            Migrations::CustomersAndSuppliers => customers_and_suppliers(manager).await?,
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Reverse order so drops never violate foreign keys.
        match self {
            Migrations::CustomersAndSuppliers => {
                for t in [
                    Suppliers::Table.into_iden(),
                    Customers::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
            Migrations::PermissionCatalog => {
                // Data, not schema: reverse it by deleting the rows this migration
                // owns, which is exactly the seeded catalog. Scoped by name so a
                // permission an operator added by hand survives a rollback. The
                // `role_permissions` foreign key cascades.
                let conn = manager.get_connection();
                for (group, actions) in PERMISSION_CATALOG {
                    for action in *actions {
                        let name = format!("{group}-{action}");
                        permissions::Entity::delete_many()
                            .filter(permissions::Column::Name.eq(name))
                            .exec(conn)
                            .await?;
                    }
                }
            }
            Migrations::SalesAndStock => {
                for t in [
                    StockMovements::Table.into_iden(),
                    SaleDetails::Table.into_iden(),
                    Sales::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
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
                .to_owned(),
        )
        .await?;

    // Non-unique indexes cannot be inlined into CREATE TABLE — `CONSTRAINT "x" ("c")`
    // is rejected by both SQLite and Postgres. Each one is its own statement.
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_item_categories_sort")
                .table(ItemCategories::Table)
                .col(ItemCategories::SortId)
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
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_items_name")
                .table(Items::Table)
                .col(Items::Name)
                .to_owned(),
        )
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Sales, sale lines, and the stock ledger
// ---------------------------------------------------------------------------

/// Sale header, its lines, and the append-only stock ledger.
///
/// `stock_movements` is the load-bearing table here. On-hand quantity is
/// *derived* as the sum of a row's `quantity` values, so there is deliberately no
/// quantity column on `items`: nothing can drift out of sync with the ledger,
/// because there is nothing to drift. `balance_after` records what the sum was
/// immediately after that row landed, which turns "the count looks wrong" into
/// "these two movements disagree".
///
/// `quantity` is **signed**: negative when stock leaves, positive when it arrives.
/// That single convention means a receipt and a sale are the same kind of row and
/// an audit never has to special-case a direction.
async fn sales_and_stock(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Sales::Table)
                .if_not_exists()
                .col(ColumnDef::new(Sales::Id).integer().not_null().auto_increment().primary_key().to_owned())
                // Human-facing reference. Derived from the primary key so it cannot
                // collide — see `commands::checkout`.
                .col(ColumnDef::new(Sales::InvoiceNo).string().not_null().unique_key())
                // `Draft` or `Completed`. Checkout writes a draft first and promotes it
                // on payment, so an app death mid-sale leaves a recoverable draft
                // instead of a half-written sale.
                .col(ColumnDef::new(Sales::Status).string().not_null().default("Draft"))
                .col(ColumnDef::new(Sales::Subtotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Sales::DiscountTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Sales::TaxTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Sales::GrandTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                // Below `grand_total` is a part-paid / credit sale; above it is cash
                // handed back as change. Both are legal, negative is not.
                .col(ColumnDef::new(Sales::PaidTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Sales::PaymentMethod).string().not_null().default("Cash"))
                // No foreign key yet: the customers table arrives in Stage 3 and
                // inventing a stub table here would fork the schema.
                .col(ColumnDef::new(Sales::CustomerId).integer().null())
                .col(ColumnDef::new(Sales::Note).string().null())
                .col(ColumnDef::new(Sales::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Sales::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_sales_created_at")
                .table(Sales::Table)
                .col(Sales::CreatedAt)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_sales_status")
                .table(Sales::Table)
                .col(Sales::Status)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(SaleDetails::Table)
                .if_not_exists()
                .col(ColumnDef::new(SaleDetails::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(SaleDetails::SaleId).integer().not_null())
                .col(ColumnDef::new(SaleDetails::ItemId).integer().not_null())
                // Snapshotted from `items.name` at sale time. A rename afterwards must
                // not rewrite what the customer was actually charged for.
                .col(ColumnDef::new(SaleDetails::ItemName).string().not_null())
                .col(ColumnDef::new(SaleDetails::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(SaleDetails::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(SaleDetails::Discount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(SaleDetails::LineTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(SaleDetails::TaxAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(SaleDetails::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                // Deleting the sale removes its lines — they have no meaning alone.
                .foreign_key(&mut ForeignKey::create().name("fk_sale_details_sale").from(SaleDetails::Table, SaleDetails::SaleId).to(Sales::Table, Sales::Id).on_delete(ForeignKeyAction::Cascade).to_owned())
                // RESTRICT, not CASCADE: deleting an item must not erase the record
                // that it was once sold. Items are soft-deleted anyway, so this only
                // fires on a hard delete.
                .foreign_key(&mut ForeignKey::create().name("fk_sale_details_item").from(SaleDetails::Table, SaleDetails::ItemId).to(Items::Table, Items::Id).on_delete(ForeignKeyAction::Restrict).to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_sale_details_sale")
                .table(SaleDetails::Table)
                .col(SaleDetails::SaleId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(StockMovements::Table)
                .if_not_exists()
                .col(ColumnDef::new(StockMovements::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(StockMovements::ItemId).integer().not_null())
                .col(ColumnDef::new(StockMovements::SaleId).integer().null())
                // Constrained vocabulary (`entities::sales::stock_movement::MovementType`)
                // stored as its string form, so the ledger stays readable in SQL.
                .col(ColumnDef::new(StockMovements::MovementType).string().not_null())
                // SIGNED: negative leaves the shelf, positive arrives. On-hand is
                // SUM(quantity) — there is no quantity column on `items` to fall out
                // of sync with.
                .col(ColumnDef::new(StockMovements::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                // Free text: receipt number, adjustment reason code, transfer note.
                .col(ColumnDef::new(StockMovements::Reference).string().null())
                // On-hand immediately after this row. Makes a discrepancy traceable to
                // one specific movement instead of to a number that drifted.
                .col(ColumnDef::new(StockMovements::BalanceAfter).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(StockMovements::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(&mut ForeignKey::create().name("fk_stock_movements_item").from(StockMovements::Table, StockMovements::ItemId).to(Items::Table, Items::Id).on_delete(ForeignKeyAction::Restrict).to_owned())
                // SET NULL, not CASCADE: a ledger row outlives the sale that caused it.
                // A return written after the sale is purged still has to be on record.
                .foreign_key(&mut ForeignKey::create().name("fk_stock_movements_sale").from(StockMovements::Table, StockMovements::SaleId).to(Sales::Table, Sales::Id).on_delete(ForeignKeyAction::SetNull).to_owned())
                .to_owned(),
        )
        .await?;

    // Non-unique indexes cannot be inlined into CREATE TABLE — `CONSTRAINT "x" ("c")`
    // is rejected by both SQLite and Postgres. Each one is its own statement.
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_stock_movements_item")
                .table(StockMovements::Table)
                .col(StockMovements::ItemId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_stock_movements_sale")
                .table(StockMovements::Table)
                .col(StockMovements::SaleId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_stock_movements_created_at")
                .table(StockMovements::Table)
                .col(StockMovements::CreatedAt)
                .to_owned(),
        )
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Permission catalog
// ---------------------------------------------------------------------------

/// Neither table stores a balance. It is derived from sales, receipts, purchases and
/// payments, for the same reason stock is a ledger: a stored balance is a
/// read-modify-write that two concurrent sales can interleave.
async fn customers_and_suppliers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Customers::Table)
                .if_not_exists()
                .col(ColumnDef::new(Customers::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Customers::Name).string().not_null())
                .col(ColumnDef::new(Customers::Code).string().null().unique_key())
                .col(ColumnDef::new(Customers::Email).string().null())
                .col(ColumnDef::new(Customers::Phone).string().null())
                .col(ColumnDef::new(Customers::Address).string().null())
                .col(ColumnDef::new(Customers::City).string().null())
                .col(ColumnDef::new(Customers::Country).string().null())
                .col(ColumnDef::new(Customers::Zip).string().null())
                .col(ColumnDef::new(Customers::TaxNumber).string().null())
                // What a customer may owe before checkout refuses the sale.
                .col(ColumnDef::new(Customers::CreditLimit).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Customers::LoyaltyPoints).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Customers::Note).string().null())
                .col(ColumnDef::new(Customers::Photo).string().null())
                .col(ColumnDef::new(Customers::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Customers::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Customers::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_customers_name")
                .table(Customers::Table)
                .col(Customers::Name)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Suppliers::Table)
                .if_not_exists()
                .col(ColumnDef::new(Suppliers::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Suppliers::Name).string().not_null())
                .col(ColumnDef::new(Suppliers::Code).string().null().unique_key())
                .col(ColumnDef::new(Suppliers::Email).string().null())
                .col(ColumnDef::new(Suppliers::Phone).string().null())
                .col(ColumnDef::new(Suppliers::Address).string().null())
                .col(ColumnDef::new(Suppliers::City).string().null())
                .col(ColumnDef::new(Suppliers::Country).string().null())
                .col(ColumnDef::new(Suppliers::Zip).string().null())
                .col(ColumnDef::new(Suppliers::TaxNumber).string().null())
                .col(ColumnDef::new(Suppliers::OpeningBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Suppliers::Note).string().null())
                .col(ColumnDef::new(Suppliers::Photo).string().null())
                .col(ColumnDef::new(Suppliers::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Suppliers::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Suppliers::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_suppliers_name")
                .table(Suppliers::Table)
                .col(Suppliers::Name)
                .to_owned(),
        )
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------

/// Every permission this app guards, as `group` -> actions.
///
/// Names are `group-action` (`item-create`, `sale-pos`), which is what the
/// reference's seeder writes into `permissions.name` and what the command guards
/// compare against. `group_name` is the second column, so the settings UI can
/// group them without parsing the name.
///
/// The catalog is seeded rather than left empty because `permission_names_in`
/// resolves against these rows: an empty table means every user holds nothing,
/// so a guard would lock out every operator — including one holding a `Master`
/// role, which bypasses the pivot but not this query.
const PERMISSION_CATALOG: &[(&str, &[&str])] = &[
    ("item", &["list", "create", "edit", "show", "destroy", "import"]),
    ("item_category", &["list", "create", "edit", "show", "destroy"]),
    ("brand", &["list", "create", "edit", "show", "destroy"]),
    ("unit", &["list", "create", "edit", "show", "destroy"]),
    ("sale", &["list", "create", "edit", "show", "destroy", "pos", "show_purchase_price"]),
    ("sale_return", &["list", "create", "edit", "show", "destroy"]),
    ("customer", &["list", "create", "edit", "show", "destroy"]),
    ("supplier", &["list", "create", "edit", "show", "destroy"]),
    ("stock", &["stock", "low_stock"]),
    ("user", &["list", "create", "edit", "show", "destroy"]),
    ("role", &["list", "create", "edit", "show", "destroy"]),
    ("setting", &["list", "create", "edit", "show", "destroy"]),
    (
        "report",
        &[
            "sale_report", "stock_report", "low_stock_report", "purchase_report",
            "expense_report", "income_report", "profit_loss_report", "tax_report",
            "customer_balance_report", "supplier_balance_report", "cash_flow_report",
        ],
    ),
];

/// Insert the catalog, skipping names that already exist.
///
/// Idempotent by design: migrations run once per database, but a name can also
/// arrive from a later migration or an operator's own setup, and `firstOrCreate`
/// in the reference behaves the same way. Existing rows are left untouched so a
/// soft-deleted permission is not silently resurrected by re-running.
async fn permission_catalog(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let conn = manager.get_connection();
    let now = chrono::Utc::now();

    for (group, actions) in PERMISSION_CATALOG {
        for action in *actions {
            let name = format!("{group}-{action}");
            let exists = permissions::Entity::find()
                .filter(permissions::Column::Name.eq(&name))
                .one(conn)
                .await?;
            if exists.is_some() {
                continue;
            }

            permissions::ActiveModel {
                name: Set(name),
                group_name: Set((*group).to_owned()),
                // Single guard — this app has no API surface separate from the
                // desktop window, so the reference's parity column is a constant.
                guard_name: Set("web".to_owned()),
                del_status: Set(DEL_LIVE.to_owned()),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(conn)
            .await?;
        }
    }

    Ok(())
}

#[derive(Iden)]
enum Customers {
    Table,
    Id,
    Name,
    Code,
    Email,
    Phone,
    Address,
    City,
    Country,
    Zip,
    TaxNumber,
    CreditLimit,
    LoyaltyPoints,
    Note,
    Photo,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Suppliers {
    Table,
    Id,
    Name,
    Code,
    Email,
    Phone,
    Address,
    City,
    Country,
    Zip,
    TaxNumber,
    OpeningBalance,
    Note,
    Photo,
    DelStatus,
    CreatedAt,
    UpdatedAt,
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

#[derive(Iden)]
enum Sales {
    Table,
    Id,
    InvoiceNo,
    Status,
    Subtotal,
    DiscountTotal,
    TaxTotal,
    GrandTotal,
    PaidTotal,
    PaymentMethod,
    CustomerId,
    Note,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum SaleDetails {
    Table,
    Id,
    SaleId,
    ItemId,
    ItemName,
    UnitPrice,
    Quantity,
    Discount,
    LineTotal,
    TaxAmount,
    CreatedAt,
}

#[derive(Iden)]
enum StockMovements {
    Table,
    Id,
    ItemId,
    SaleId,
    MovementType,
    Quantity,
    Reference,
    BalanceAfter,
    CreatedAt,
}



