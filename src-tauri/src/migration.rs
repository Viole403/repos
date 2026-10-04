//! Schema migrations.
//!
//! Mirrors the reference product's role/permission model (spatie-style pivots)
//! so the authorization semantics carry over, plus the master-data tables the
//! catalog and POS screens depend on.
//!
//! Money is `Decimal` (three decimal places) and quantities are `Decimal` too —
//! the reference casts both to `decimal:3`, and fractional quantities are real.

use chrono::NaiveDateTime;
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
            Box::new(Migrations::CustomersAndSuppliers),
            Box::new(Migrations::TradeCredit),
            Box::new(Migrations::SalePayments),
            Box::new(Migrations::SaleReturns),
            Box::new(Migrations::Registers),
            Box::new(Migrations::Quotations),
            Box::new(Migrations::Bookings),
            Box::new(Migrations::Promotions),
            Box::new(Migrations::Combos),
            Box::new(Migrations::SaleOrderType),
            Box::new(Migrations::Installments),
            Box::new(Migrations::InstallmentStockLink),
            Box::new(Migrations::InstallmentDownMethod),
            Box::new(Migrations::WarrantyAndServicing),
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
    TradeCredit,
    SalePayments,
    SaleReturns,
    Registers,
    Quotations,
    Bookings,
    Promotions,
    Combos,
    SaleOrderType,
    Installments,
    InstallmentStockLink,
    InstallmentDownMethod,
    WarrantyAndServicing,
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

/// A UTC timestamp as `NaiveDateTime`, because that is what a `timestamp` column is.
///
/// `DateTimeUtc` would be the obvious type, and it is wrong: sqlx maps it to
/// `TIMESTAMPTZ`, so reading a `TIMESTAMP` column into it fails at runtime on
/// Postgres — `mismatched types` during the migration's own `SELECT`. `NaiveDateTime`
/// maps to `TIMESTAMP`, which is exactly what the column is.
pub fn now() -> NaiveDateTime {
    chrono::Utc::now().naive_utc()
}

/// Money/quantity precision: 18 total digits, 3 after the point.
///
/// `ColumnType::Decimal(None)` renders as the literal string `real_decimal` on SQLite,
/// which is not valid DDL — the migration fails with `near "(": syntax error`. Passing
/// an explicit precision makes sea-query emit `real(18,3)`, which every backend accepts.
/// The reference casts money to `decimal:3`, so scale 3 is deliberate.
const DECIMAL_PRECISION: u32 = 18;
pub(crate) const DECIMAL_SCALE: u32 = 3;

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
            Migrations::TradeCredit => "trade_credit",
            Migrations::SalePayments => "sale_payments",
            Migrations::SaleReturns => "sale_returns",
        Migrations::Registers => "registers",
        Migrations::Quotations => "quotations",
        Migrations::Bookings => "bookings",
        Migrations::Promotions => "promotions",
        Migrations::Combos => "combos",
        Migrations::SaleOrderType => "sale_order_type",
        Migrations::Installments => "installments",
        Migrations::InstallmentStockLink => "installment_stock_link",
        Migrations::InstallmentDownMethod => "installment_down_method",
        Migrations::WarrantyAndServicing => "warranty_and_servicing",
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
            Migrations::TradeCredit => trade_credit(manager).await?,
            Migrations::SalePayments => sale_payments(manager).await?,
            Migrations::SaleReturns => sale_returns(manager).await?,
            Migrations::Registers => registers(manager).await?,
            Migrations::Quotations => quotations(manager).await?,
            Migrations::Bookings => bookings(manager).await?,
            Migrations::Promotions => promotions(manager).await?,
            Migrations::Combos => combos(manager).await?,
            Migrations::SaleOrderType => sale_order_type(manager).await?,
            Migrations::Installments => installments(manager).await?,
            Migrations::InstallmentStockLink => installment_stock_link(manager).await?,
            Migrations::InstallmentDownMethod => installment_down_method(manager).await?,
            Migrations::WarrantyAndServicing => warranty_and_servicing(manager).await?,
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Reverse order so drops never violate foreign keys.
        match self {
            Migrations::WarrantyAndServicing => {
                for t in [Servicings::Table.into_iden(), Warranties::Table.into_iden()] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
                let conn = manager.get_connection();
                for name in WARRANTY_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::InstallmentDownMethod => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(InstallmentSales::Table)
                            .drop_column(InstallmentSales::DownPaymentMethod)
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::InstallmentStockLink => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(StockMovements::Table)
                            .drop_column(StockMovements::InstallmentSaleId)
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::Installments => {
                for t in [
                    InstallmentSaleDetails::Table.into_iden(),
                    InstallmentSales::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
                let conn = manager.get_connection();
                for name in INSTALLMENT_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::SaleOrderType => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Sales::Table)
                            .drop_column(Sales::OrderType)
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::Combos => {
                for t in [
                    ComboSales::Table.into_iden(),
                    ComboItems::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
            Migrations::Promotions => {
                manager.drop_table(Table::drop().table(Promotions::Table).if_exists().to_owned()).await?;
                let conn = manager.get_connection();
                for name in PROMOTION_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::Bookings => {
                manager.drop_table(Table::drop().table(Bookings::Table).if_exists().to_owned()).await?;
                let conn = manager.get_connection();
                for name in BOOKING_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::Quotations => {
                for t in [
                    QuotationDetails::Table.into_iden(),
                    Quotations::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
                let conn = manager.get_connection();
                for name in QUOTATION_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::Registers => {
                manager.drop_table(Table::drop().table(Registers::Table).if_exists().to_owned()).await?;
                // Same ownership rule as the catalog arm below: only the rows this
                // migration seeded.
                let conn = manager.get_connection();
                for name in REGISTER_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::SaleReturns => {
                for t in [
                    SaleReturnDetails::Table.into_iden(),
                    SaleReturns::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
            Migrations::SalePayments => {
                manager
                    .drop_table(Table::drop().table(SalePayments::Table).if_exists().to_owned())
                    .await?;
            }
            Migrations::TradeCredit => {
                for t in [
                    SupplierPayments::Table.into_iden(),
                    CustomerReceives::Table.into_iden(),
                ] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
            }
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

/// Goods handed back against a sale.
///
/// A return is its own document rather than an edit of the sale: the sale is financial
/// history and stays as it was, and the return is what reverses it. Nothing here
/// subtracts from `sales` — the money comes off the customer through a receipt.
/// One cashier shift. Scoped to the user only: outlets and counters arrive in
/// Stage 9, and columns pointing at tables that do not exist yet would fork the
/// schema for existing databases.
///
/// No `del_status`: a closed shift is immutable history, and an open one is closed
/// rather than deleted — the same reason `sales` carries none.
async fn registers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Registers::Table)
                .if_not_exists()
                .col(ColumnDef::new(Registers::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Registers::UserId).integer().not_null())
                // 'Open' or 'Closed'. Strings, like every other status here.
                .col(ColumnDef::new(Registers::Status).string().not_null())
                .col(ColumnDef::new(Registers::OpenedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Registers::ClosedAt).custom(TIMESTAMP).null())
                .col(ColumnDef::new(Registers::OpeningBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                // Per-method opening float as JSON, mirroring the reference's
                // `opening_details`: `[{"method":"Cash","amount":"50000.000"}]`.
                .col(ColumnDef::new(Registers::OpeningDetails).text().null())
                // What the cashier counted at close; expected is the snapshot below.
                .col(ColumnDef::new(Registers::ClosingBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Registers::ExpectedBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Registers::Note).string().null())
                .col(ColumnDef::new(Registers::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_registers_user")
                        .from(Registers::Table, Registers::UserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_registers_user_id")
                .table(Registers::Table)
                .col(Registers::UserId)
                .to_owned(),
        )
        .await?;

    // Same insert-if-absent shape as the catalog seeder, so re-running leaves an
    // operator's own setup alone.
    let conn = manager.get_connection();
    let now = now();
    for name in REGISTER_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            group_name: Set("register".to_owned()),
            guard_name: Set("web".to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    Ok(())
}

async fn sale_returns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(SaleReturns::Table)
                .if_not_exists()
                .col(ColumnDef::new(SaleReturns::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(SaleReturns::SaleId).integer().not_null())
                // Derived from this row's own primary key, for the same reason
                // `sales.invoice_no` is: `MAX() + 1` collides under concurrency.
                .col(ColumnDef::new(SaleReturns::ReturnNo).string().not_null().unique_key())
                .col(ColumnDef::new(SaleReturns::Reason).string().not_null())
                .col(ColumnDef::new(SaleReturns::RefundedTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(SaleReturns::ReturnedBy).integer().null())
                .col(ColumnDef::new(SaleReturns::Note).string().null())
                .col(ColumnDef::new(SaleReturns::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_sale_returns_sale")
                        .from(SaleReturns::Table, SaleReturns::SaleId)
                        .to(Sales::Table, Sales::Id)
                        // Restrict: a sale is financial history and is never deleted, so
                        // this can only fire if that rule is broken.
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_sale_returns_sale_id")
                .table(SaleReturns::Table)
                .col(SaleReturns::SaleId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(SaleReturnDetails::Table)
                .if_not_exists()
                .col(ColumnDef::new(SaleReturnDetails::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(SaleReturnDetails::SaleReturnId).integer().not_null())
                // The line being returned, so "already returned" is a sum over these
                // rather than a number someone has to keep in step.
                .col(ColumnDef::new(SaleReturnDetails::SaleDetailId).integer().not_null())
                .col(ColumnDef::new(SaleReturnDetails::ItemId).integer().not_null())
                .col(ColumnDef::new(SaleReturnDetails::ItemName).string().not_null())
                .col(ColumnDef::new(SaleReturnDetails::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(SaleReturnDetails::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(SaleReturnDetails::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_sale_return_details_return")
                        .from(SaleReturnDetails::Table, SaleReturnDetails::SaleReturnId)
                        .to(SaleReturns::Table, SaleReturns::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    Ok(())
}

/// A customer appointment: who, with which staff member, when. No outlet column —
/// outlets arrive in Stage 9 and columns pointing at missing tables fork the schema.
async fn bookings(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Bookings::Table)
                .if_not_exists()
                .col(ColumnDef::new(Bookings::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Bookings::CustomerId).integer().not_null())
                .col(ColumnDef::new(Bookings::ServiceSellerId).integer().null())
                .col(ColumnDef::new(Bookings::CreatedBy).integer().null())
                .col(ColumnDef::new(Bookings::Status).string().not_null().default("Booked"))
                .col(ColumnDef::new(Bookings::StartAt).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(Bookings::EndAt).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(Bookings::Note).string().null())
                .col(ColumnDef::new(Bookings::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Bookings::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Bookings::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_bookings_customer")
                        .from(Bookings::Table, Bookings::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_bookings_service_seller")
                        .from(Bookings::Table, Bookings::ServiceSellerId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_bookings_start_at")
                .table(Bookings::Table)
                .col(Bookings::StartAt)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in BOOKING_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            group_name: Set("booking".to_owned()),
            guard_name: Set("web".to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    Ok(())
}

/// How the sale leaves the shop: counter, pickup, delivery or online. A column on
/// the sale rather than a master table, like booking status and return reason —
/// the vocabulary is closed and the commands enforce it.
async fn sale_order_type(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Sales::Table)
                .add_column(
                    ColumnDef::new(Sales::OrderType)
                        .string()
                        .not_null()
                        .default("InStore")
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Links the stock ledger to installment sales. The goods leave the shelf the day
/// the credit sale is written — not when the last due clears — so the handover
/// writes a negative ledger row exactly like a counter sale does.
async fn installment_stock_link(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(StockMovements::Table)
                .add_column(
                    ColumnDef::new(StockMovements::InstallmentSaleId).integer().null().to_owned(),
                )
                .to_owned(),
        )
        .await?;

    // SET NULL, not CASCADE: a ledger row outlives the credit sale that caused it.
    // SQLite cannot add a foreign key to an existing table at all — sea-query
    // panics — so only Postgres gets the constraint. The column and the index
    // apply everywhere; the application always writes a valid id or NULL.
    if manager.get_database_backend() != sea_orm::DbBackend::Sqlite {
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_stock_movements_installment_sale")
                    .from(StockMovements::Table, StockMovements::InstallmentSaleId)
                    .to(InstallmentSales::Table, InstallmentSales::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;
    }

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_stock_movements_installment_sale_id")
                .table(StockMovements::Table)
                .col(StockMovements::InstallmentSaleId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Repair tickets and paid repair jobs. A warranty is a product sent back through
/// the pipeline (customer → vendor → customer); a servicing is a repair the shop
/// bills for. Both name the product as free text like the reference — the unit on
/// the bench is not necessarily a catalog row anymore.
async fn warranty_and_servicing(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Warranties::Table)
                .if_not_exists()
                .col(ColumnDef::new(Warranties::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Warranties::CustomerId).integer().not_null())
                .col(ColumnDef::new(Warranties::ProductName).string().not_null())
                .col(ColumnDef::new(Warranties::ProductSerialNo).string().null())
                .col(ColumnDef::new(Warranties::Description).text().null())
                .col(ColumnDef::new(Warranties::ReceivingDate).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(Warranties::DeliveryDate).custom(TIMESTAMP).null())
                .col(ColumnDef::new(Warranties::CurrentStatus).string().not_null().default("R_F_C"))
                .col(ColumnDef::new(Warranties::TechnicianId).integer().null())
                .col(ColumnDef::new(Warranties::PresentLocation).string().null())
                .col(ColumnDef::new(Warranties::SenderServiceCenter).string().null())
                .col(ColumnDef::new(Warranties::ReceiverServiceCenter).string().null())
                .col(ColumnDef::new(Warranties::CreatedBy).integer().null())
                .col(ColumnDef::new(Warranties::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Warranties::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Warranties::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_warranties_customer")
                        .from(Warranties::Table, Warranties::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_warranties_technician")
                        .from(Warranties::Table, Warranties::TechnicianId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Servicings::Table)
                .if_not_exists()
                .col(ColumnDef::new(Servicings::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Servicings::CustomerId).integer().not_null())
                .col(ColumnDef::new(Servicings::ProductName).string().not_null())
                .col(ColumnDef::new(Servicings::ProductModel).string().null())
                .col(ColumnDef::new(Servicings::ProblemDescription).text().null())
                .col(ColumnDef::new(Servicings::ReceivingDate).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(Servicings::DeliveryDate).custom(TIMESTAMP).null())
                .col(ColumnDef::new(Servicings::ServicingCharge).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(Servicings::PaidAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Servicings::CurrentStatus).string().not_null().default("Received"))
                .col(ColumnDef::new(Servicings::TechnicianId).integer().null())
                .col(ColumnDef::new(Servicings::CreatedBy).integer().null())
                .col(ColumnDef::new(Servicings::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Servicings::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Servicings::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_servicings_customer")
                        .from(Servicings::Table, Servicings::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_servicings_technician")
                        .from(Servicings::Table, Servicings::TechnicianId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_warranties_customer_id")
                .table(Warranties::Table)
                .col(Warranties::CustomerId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_servicings_customer_id")
                .table(Servicings::Table)
                .col(Servicings::CustomerId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in WARRANTY_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        // The catalog test requires `group_name` to be the name's prefix, which
        // is what the settings UI groups on.
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("service");
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            group_name: Set(group.to_owned()),
            guard_name: Set("web".to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }
    Ok(())
}

/// The down payment's tender method, so a cash down payment can later count
/// toward the register's expected cash. Stored beside the amount rather than as
/// a schedule row, mirroring the reference — which keeps it on the header too.
async fn installment_down_method(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(InstallmentSales::Table)
                .add_column(
                    ColumnDef::new(InstallmentSales::DownPaymentMethod)
                        .string()
                        .null()
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// A credit sale paid off over time: one item handed over now, the balance split
/// into dated schedule rows. Two tables, not three — the reference's
/// `InstallmentSalePayment` is never written (only a commented-out read), so the
/// schedule rows carry their own `paid_amount` and there is nowhere else a payment
/// could hide.
///
/// Paid, due and status are derived (`down_payment + SUM(paid_amount)`), never
/// stored: the reference stores them and recomputes them on every payment anyway,
/// which is two sources for one figure. The schedule is auto-split for now —
// floor division with the remainder on the first due, like the reference —
// manual per-due amounts are still open.
async fn installments(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(InstallmentSales::Table)
                .if_not_exists()
                .col(ColumnDef::new(InstallmentSales::Id).integer().not_null().auto_increment().primary_key().to_owned())
                // Written as a placeholder, then replaced with `INST-{id:06}` — same
                // derivation as the sale invoice number, so it cannot collide.
                .col(ColumnDef::new(InstallmentSales::ReferenceNo).string().not_null())
                .col(ColumnDef::new(InstallmentSales::CustomerId).integer().not_null())
                .col(ColumnDef::new(InstallmentSales::ItemId).integer().not_null())
                .col(ColumnDef::new(InstallmentSales::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(InstallmentSales::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(InstallmentSales::DiscountAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSales::InterestPercent).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSales::InterestAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSales::OtherCharges).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSales::Total).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(InstallmentSales::DownPayment).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSales::NumberOfInstallments).integer().not_null())
                .col(ColumnDef::new(InstallmentSales::IntervalDays).integer().not_null())
                .col(ColumnDef::new(InstallmentSales::CreatedBy).integer().null())
                .col(ColumnDef::new(InstallmentSales::Note).string().null())
                .col(ColumnDef::new(InstallmentSales::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(InstallmentSales::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(InstallmentSales::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_installment_sales_customer")
                        .from(InstallmentSales::Table, InstallmentSales::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                // RESTRICT, not CASCADE: deleting an item must not erase the record
                // that it was once sold on credit.
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_installment_sales_item")
                        .from(InstallmentSales::Table, InstallmentSales::ItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(InstallmentSaleDetails::Table)
                .if_not_exists()
                .col(ColumnDef::new(InstallmentSaleDetails::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(InstallmentSaleDetails::InstallmentSaleId).integer().not_null())
                // Midnight on the due day: a due date is a date, and every timestamp
                // here already means "that day at midnight" when it has to.
                .col(ColumnDef::new(InstallmentSaleDetails::DueDate).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(InstallmentSaleDetails::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(InstallmentSaleDetails::PaidAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSaleDetails::PaidDate).custom(TIMESTAMP).null())
                .col(ColumnDef::new(InstallmentSaleDetails::PaymentMethod).string().null())
                .col(ColumnDef::new(InstallmentSaleDetails::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(InstallmentSaleDetails::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_installment_sale_details_sale")
                        .from(InstallmentSaleDetails::Table, InstallmentSaleDetails::InstallmentSaleId)
                        .to(InstallmentSales::Table, InstallmentSales::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_installment_sales_customer_id")
                .table(InstallmentSales::Table)
                .col(InstallmentSales::CustomerId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_installment_sale_details_sale_id")
                .table(InstallmentSaleDetails::Table)
                .col(InstallmentSaleDetails::InstallmentSaleId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in INSTALLMENT_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            group_name: Set("installment".to_owned()),
            guard_name: Set("web".to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }
    Ok(())
}

/// A catalog bundle and its per-sale explosion record. The bundle sells as one
/// line at the bundle's price; the components move the stock. `combo_sales` is
/// the audit of that explosion, so a receipt can name what the bundle contained
/// and a return can put the right stock back.
async fn combos(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(ComboItems::Table)
                .if_not_exists()
                .col(ColumnDef::new(ComboItems::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ComboItems::ComboItemId).integer().not_null())
                .col(ColumnDef::new(ComboItems::ItemId).integer().not_null())
                .col(ColumnDef::new(ComboItems::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_combo_items_combo")
                        .from(ComboItems::Table, ComboItems::ComboItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_combo_items_item")
                        .from(ComboItems::Table, ComboItems::ItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .unique()
                .name("uq_combo_items_pair")
                .table(ComboItems::Table)
                .col(ComboItems::ComboItemId)
                .col(ComboItems::ItemId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ComboSales::Table)
                .if_not_exists()
                .col(ColumnDef::new(ComboSales::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ComboSales::SaleId).integer().not_null())
                .col(ColumnDef::new(ComboSales::SaleDetailId).integer().not_null())
                .col(ColumnDef::new(ComboSales::ComboItemId).integer().not_null())
                .col(ColumnDef::new(ComboSales::ItemId).integer().not_null())
                .col(ColumnDef::new(ComboSales::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_combo_sales_sale")
                        .from(ComboSales::Table, ComboSales::SaleId)
                        .to(Sales::Table, Sales::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_combo_sales_detail")
                        .from(ComboSales::Table, ComboSales::SaleDetailId)
                        .to(SaleDetails::Table, SaleDetails::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_combo_sales_detail_id")
                .table(ComboSales::Table)
                .col(ComboSales::SaleDetailId)
                .to_owned(),
        )
        .await?;

    Ok(())
}

/// Discount rules the till applies by itself. Four kinds, one table: the columns a
/// kind does not use stay null, and the commands refuse a row whose kind and
/// columns disagree — a second table would only move that check, not remove it.
async fn promotions(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Promotions::Table)
                .if_not_exists()
                .col(ColumnDef::new(Promotions::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Promotions::Title).string().not_null())
                // ItemPercent, ItemFixed, OrderPercent, OrderFixed, BuyGet.
                .col(ColumnDef::new(Promotions::Kind).string().not_null())
                .col(ColumnDef::new(Promotions::TargetItemId).integer().null())
                .col(ColumnDef::new(Promotions::RewardItemId).integer().null())
                .col(ColumnDef::new(Promotions::Percent).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Promotions::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Promotions::MinTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Promotions::BuyQty).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Promotions::GetQty).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Promotions::StartAt).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(Promotions::EndAt).custom(TIMESTAMP).not_null())
                .col(ColumnDef::new(Promotions::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Promotions::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Promotions::UpdatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_promotions_target_item")
                        .from(Promotions::Table, Promotions::TargetItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_promotions_reward_item")
                        .from(Promotions::Table, Promotions::RewardItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_promotions_target_item_id")
                .table(Promotions::Table)
                .col(Promotions::TargetItemId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in PROMOTION_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            group_name: Set("promotion".to_owned()),
            guard_name: Set("web".to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    Ok(())
}

/// A price offer to a customer. Moves no stock and takes no payment — that is
/// what separates it from a draft, which is a sale waiting to happen. Totals are
/// derived from the lines by the commands, never trusted from the client.
async fn quotations(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Quotations::Table)
                .if_not_exists()
                .col(ColumnDef::new(Quotations::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Quotations::CustomerId).integer().not_null())
                .col(ColumnDef::new(Quotations::QuotationNo).string().not_null().unique_key())
                // Midnight of the quoted day. A date, not an instant: `TIMESTAMP` is
                // the portable column and midnight UTC keeps ordering sane.
                .col(ColumnDef::new(Quotations::QuotedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Quotations::ReferenceNo).string().null())
                .col(ColumnDef::new(Quotations::Subtotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Quotations::DiscountTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Quotations::GrandTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Quotations::CreatedBy).integer().null())
                .col(ColumnDef::new(Quotations::Note).string().null())
                .col(ColumnDef::new(Quotations::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_quotations_customer")
                        .from(Quotations::Table, Quotations::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_quotations_customer_id")
                .table(Quotations::Table)
                .col(Quotations::CustomerId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(QuotationDetails::Table)
                .if_not_exists()
                .col(ColumnDef::new(QuotationDetails::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(QuotationDetails::QuotationId).integer().not_null())
                .col(ColumnDef::new(QuotationDetails::ItemId).integer().not_null())
                // Snapshot, like every other line table: a rename must not rewrite
                // an issued offer.
                .col(ColumnDef::new(QuotationDetails::ItemName).string().not_null())
                .col(ColumnDef::new(QuotationDetails::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(QuotationDetails::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(QuotationDetails::Discount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(QuotationDetails::LineTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_quotation_details_quotation")
                        .from(QuotationDetails::Table, QuotationDetails::QuotationId)
                        .to(Quotations::Table, Quotations::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in QUOTATION_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            group_name: Set("quotation".to_owned()),
            guard_name: Set("web".to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    Ok(())
}

/// How one sale was paid, once per tender.
///
/// A sale paid half card and half cash writes two rows. `sales.paid_total` stays as the
/// single figure queries read; this table is the detail behind it, so "which card" and
/// "which QRIS reference" are answerable after the fact rather than lost in one string.
async fn sale_payments(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(SalePayments::Table)
                .if_not_exists()
                .col(ColumnDef::new(SalePayments::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(SalePayments::SaleId).integer().not_null())
                .col(ColumnDef::new(SalePayments::Method).string().not_null())
                .col(ColumnDef::new(SalePayments::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                // Gateway reference, receipt number, or whatever the tender produces.
                .col(ColumnDef::new(SalePayments::Reference).string().null())
                .col(ColumnDef::new(SalePayments::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_sale_payments_sale")
                        .from(SalePayments::Table, SalePayments::SaleId)
                        .to(Sales::Table, Sales::Id)
                        // A sale is financial history and is never deleted, so this can
                        // cascade without losing anything. It exists so a torn-down draft
                        // does not leave tenders pointing at nothing.
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_sale_payments_sale_id")
                .table(SalePayments::Table)
                .col(SalePayments::SaleId)
                .to_owned(),
        )
        .await?;

    Ok(())
}

/// What a customer paid and what was paid to a supplier.
///
/// Append-only, like `stock_movements`: a balance is the sum over these and the
/// sales or purchases that made it owed. No running balance column, because that is a
/// read-modify-write two concurrent payments can interleave.
async fn trade_credit(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(CustomerReceives::Table)
                .if_not_exists()
                .col(ColumnDef::new(CustomerReceives::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(CustomerReceives::CustomerId).integer().not_null())
                .col(ColumnDef::new(CustomerReceives::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(CustomerReceives::Reference).string().null())
                // When the money arrived, which is not when the row was written: a
                // receipt for last Tuesday is entered today.
                .col(ColumnDef::new(CustomerReceives::PaidAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(CustomerReceives::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_customer_receives_customer")
                        .from(CustomerReceives::Table, CustomerReceives::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        // Restricted rather than cascading: deleting a customer who has
                        // paid is a mistake worth refusing, not something to tidy up.
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(SupplierPayments::Table)
                .if_not_exists()
                .col(ColumnDef::new(SupplierPayments::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(SupplierPayments::SupplierId).integer().not_null())
                .col(ColumnDef::new(SupplierPayments::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(SupplierPayments::Reference).string().null())
                .col(ColumnDef::new(SupplierPayments::PaidAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(SupplierPayments::CreatedAt).custom(TIMESTAMP).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_supplier_payments_supplier")
                        .from(SupplierPayments::Table, SupplierPayments::SupplierId)
                        .to(Suppliers::Table, Suppliers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
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
    let now = now();

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
enum ComboItems {
    Table,
    Id,
    ComboItemId,
    ItemId,
    Quantity,
}

#[derive(Iden)]
enum ComboSales {
    Table,
    Id,
    SaleId,
    SaleDetailId,
    ComboItemId,
    ItemId,
    Quantity,
}

#[derive(Iden)]
enum Promotions {
    Table,
    Id,
    Title,
    Kind,
    TargetItemId,
    RewardItemId,
    Percent,
    Amount,
    MinTotal,
    BuyQty,
    GetQty,
    StartAt,
    EndAt,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

const PROMOTION_PERMISSIONS: &[&str] = &[
    "promotion-list",
    "promotion-create",
    "promotion-edit",
    "promotion-show",
    "promotion-destroy",
];

#[derive(Iden)]
enum InstallmentSales {
    Table,
    Id,
    ReferenceNo,
    CustomerId,
    ItemId,
    Quantity,
    UnitPrice,
    DiscountAmount,
    InterestPercent,
    InterestAmount,
    OtherCharges,
    Total,
    DownPayment,
    DownPaymentMethod,
    NumberOfInstallments,
    IntervalDays,
    CreatedBy,
    Note,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum InstallmentSaleDetails {
    Table,
    Id,
    InstallmentSaleId,
    DueDate,
    Amount,
    PaidAmount,
    PaidDate,
    PaymentMethod,
    DelStatus,
    CreatedAt,
}

/// Permissions this migration owns, for the down arm above.
const INSTALLMENT_PERMISSIONS: &[&str] = &[
    "installment-list",
    "installment-create",
    "installment-show",
    "installment-collect",
];

#[derive(Iden)]
enum Warranties {
    Table,
    Id,
    CustomerId,
    ProductName,
    ProductSerialNo,
    Description,
    ReceivingDate,
    DeliveryDate,
    CurrentStatus,
    TechnicianId,
    PresentLocation,
    SenderServiceCenter,
    ReceiverServiceCenter,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Servicings {
    Table,
    Id,
    CustomerId,
    ProductName,
    ProductModel,
    ProblemDescription,
    ReceivingDate,
    DeliveryDate,
    ServicingCharge,
    PaidAmount,
    CurrentStatus,
    TechnicianId,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

/// Permissions this migration owns, for the down arm above.
const WARRANTY_PERMISSIONS: &[&str] = &[
    "warranty-list",
    "warranty-create",
    "warranty-show",
    "warranty-status",
    "servicing-list",
    "servicing-create",
    "servicing-show",
    "servicing-collect",
];

#[derive(Iden)]
enum Bookings {
    Table,
    Id,
    CustomerId,
    ServiceSellerId,
    CreatedBy,
    Status,
    StartAt,
    EndAt,
    Note,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

/// Permissions this migration owns, for the down arm above.
const BOOKING_PERMISSIONS: &[&str] = &[
    "booking-list",
    "booking-create",
    "booking-edit",
    "booking-show",
    "booking-destroy",
];

#[derive(Iden)]
enum Quotations {
    Table,
    Id,
    CustomerId,
    QuotationNo,
    QuotedAt,
    ReferenceNo,
    Subtotal,
    DiscountTotal,
    GrandTotal,
    CreatedBy,
    Note,
    CreatedAt,
}

#[derive(Iden)]
enum QuotationDetails {
    Table,
    Id,
    QuotationId,
    ItemId,
    ItemName,
    Quantity,
    UnitPrice,
    Discount,
    LineTotal,
}

/// Permissions this migration owns, for the down arm above.
const QUOTATION_PERMISSIONS: &[&str] = &[
    "quotation-list",
    "quotation-create",
    "quotation-edit",
    "quotation-show",
    "quotation-destroy",
];

#[derive(Iden)]
enum Registers {
    Table,
    Id,
    UserId,
    Status,
    OpenedAt,
    ClosedAt,
    OpeningBalance,
    OpeningDetails,
    ClosingBalance,
    ExpectedBalance,
    Note,
    CreatedAt,
}

/// Permissions this migration owns, for the down arm above.
const REGISTER_PERMISSIONS: &[&str] = &[
    "register-open",
    "register-close",
    "register-summary",
    "register-list",
];

#[derive(Iden)]
enum SaleReturns {
    Table,
    Id,
    SaleId,
    ReturnNo,
    Reason,
    RefundedTotal,
    ReturnedBy,
    Note,
    CreatedAt,
}

#[derive(Iden)]
enum SaleReturnDetails {
    Table,
    Id,
    SaleReturnId,
    SaleDetailId,
    ItemId,
    ItemName,
    Quantity,
    UnitPrice,
    Amount,
}

#[derive(Iden)]
enum SalePayments {
    Table,
    Id,
    SaleId,
    Method,
    Amount,
    Reference,
    CreatedAt,
}

#[derive(Iden)]
enum CustomerReceives {
    Table,
    Id,
    CustomerId,
    Amount,
    Reference,
    PaidAt,
    CreatedAt,
}

#[derive(Iden)]
enum SupplierPayments {
    Table,
    Id,
    SupplierId,
    Amount,
    Reference,
    PaidAt,
    CreatedAt,
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
    OrderType,
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
    InstallmentSaleId,
    MovementType,
    Quantity,
    Reference,
    BalanceAfter,
    CreatedAt,
}



