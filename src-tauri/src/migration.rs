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
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table, TableForeignKey};

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DbBackend, EntityTrait, QueryFilter};

use crate::entities::auth::permissions;
use crate::entities::sales::{sale, sale_payment};
use crate::entities::trade::payment_method;

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
            Box::new(Migrations::GiftCards),
            Box::new(Migrations::Loyalty),
            Box::new(Migrations::ServiceRatings),
            Box::new(Migrations::SaleRounding),
            Box::new(Migrations::CreditNotes),
            Box::new(Migrations::ManagerApprovals),
            Box::new(Migrations::ItemSubCategories),
            Box::new(Migrations::ItemVariationDepth),
            Box::new(Migrations::ItemBatches),
            Box::new(Migrations::FixedAssets),
            Box::new(Migrations::LoyaltyPointsWidth),
            Box::new(Migrations::Purchases),
            Box::new(Migrations::Employees),
            Box::new(Migrations::Accounting),
            Box::new(Migrations::TenderReferences),
            Box::new(Migrations::RecurringExpenses),
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
    GiftCards,
    Loyalty,
    ServiceRatings,
    SaleRounding,
    CreditNotes,
    ManagerApprovals,
    ItemSubCategories,
    ItemVariationDepth,
    ItemBatches,
    FixedAssets,
    LoyaltyPointsWidth,
    Purchases,
    Employees,
    Accounting,
    TenderReferences,
    RecurringExpenses,
}

/// Soft-delete marker used across the reference's tables.
const DEL_LIVE: &str = "Live";

/// Timestamp column type, per backend.
///
/// `ColumnType::Timestamp` renders as `timestamp_text` on SQLite — not valid SQLite
/// DDL, so the migration fails with `near "(": syntax error` — and the type is
/// therefore spelled as raw SQL. But there is **no single spelling that works
/// everywhere**, and picking one that appears to is how a portability claim goes
/// untested until someone runs it:
///
/// - SQLite accepts any type name, so it never distinguishes these.
/// - Postgres wants `timestamp`; `datetime` does not exist and the migration fails
///   with `type "datetime" does not exist`.
/// - MySQL wants `datetime`. sqlx maps `NaiveDateTime` to `DATETIME`, so a column
///   created as `TIMESTAMP` cannot be decoded at all — `mismatched types` while
///   reading back the first row. MySQL's `TIMESTAMP` also carries a 2038 range limit
///   and implicit NOT NULL, neither of which a created-at column wants.
///
/// The constant is therefore resolved from the connection's backend rather than
/// fixed, and call sites read `ts` — a local resolved once per migration from
/// `manager.get_database_backend()`, not a process global, because the suite
/// exercises several backends inside one process.
///
/// Both halves of this were found by *running* the server-backed legs. `cargo
/// check` and the SQLite suite are green with the wrong constant either way.
fn timestamp_type(backend: sea_orm::DbBackend) -> &'static str {
    match backend {
        sea_orm::DbBackend::MySql => "datetime",
        _ => "timestamp",
    }
}

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
        Migrations::GiftCards => "gift_cards",
        Migrations::Loyalty => "loyalty",
        Migrations::ServiceRatings => "service_ratings",
        Migrations::SaleRounding => "sale_rounding",
        Migrations::CreditNotes => "credit_notes",
        Migrations::ManagerApprovals => "manager_approvals",
        Migrations::ItemSubCategories => "item_sub_categories",
        Migrations::ItemVariationDepth => "item_variation_depth",
        Migrations::ItemBatches => "item_batches",
        Migrations::FixedAssets => "fixed_assets",
        Migrations::LoyaltyPointsWidth => "loyalty_points_width",
Migrations::Purchases => "purchases",
            Migrations::Employees => "employees",
            Migrations::Accounting => "accounting",
        Migrations::TenderReferences => "tender_references",
        Migrations::RecurringExpenses => "recurring_expenses",
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
            Migrations::GiftCards => gift_cards(manager).await?,
            Migrations::Loyalty => loyalty(manager).await?,
            Migrations::ServiceRatings => service_ratings(manager).await?,
            Migrations::SaleRounding => sale_rounding(manager).await?,
            Migrations::CreditNotes => credit_notes(manager).await?,
            Migrations::ManagerApprovals => manager_approvals(manager).await?,
            Migrations::ItemSubCategories => item_sub_categories(manager).await?,
            Migrations::ItemVariationDepth => item_variation_depth(manager).await?,
            Migrations::ItemBatches => item_batches(manager).await?,
            Migrations::FixedAssets => fixed_assets(manager).await?,
            Migrations::LoyaltyPointsWidth => loyalty_points_width(manager).await?,
            Migrations::Purchases => purchases(manager).await?,
            Migrations::Employees => employees(manager).await?,
            Migrations::Accounting => accounting(manager).await?,
            Migrations::TenderReferences => tender_references(manager).await?,
            Migrations::RecurringExpenses => recurring_expenses(manager).await?,
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Reverse order so drops never violate foreign keys.
        match self {
            Migrations::RecurringExpenses => {
                let tables: [DynIden; 1] = [ExpenseRecurrings::Table.into_iden()];
                for table in tables {
                    manager
                        .drop_table(Table::drop().table(table).if_exists().to_owned())
                        .await?;
                }
                manager
                    .alter_table(
                        Table::alter()
                            .table(Expenses::Table)
                            .drop_column(Expenses::RecurringExpenseId)
                            .to_owned(),
                    )
                    .await?;
                let conn = manager.get_connection();
                for name in RECURRING_PERMISSIONS {
                    let row = permissions::Entity::find()
                        .filter(permissions::Column::Name.eq(*name))
                        .one(conn)
                        .await?;
                    if let Some(row) = row {
                        permissions::Entity::delete_by_id(row.id).exec(conn).await?;
                    }
                }
            }
            Migrations::Employees => {
                // The accounting tables referenced `users` here; see the note on
                // `employees` about why the forward reference cannot be fixed after
                // the fact.
                manager
                    .drop_table(Table::drop().table(Employees::Table).if_exists().to_owned())
                    .await?;
            }
            Migrations::TenderReferences => {
                // One statement per change: SQLite cannot apply several alter
                // options in a single `ALTER`.
                manager
                    .alter_table(
                        Table::alter()
                            .table(Sales::Table)
                            .drop_column(Sales::PaymentMethodId)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(CustomerReceives::Table)
                            .drop_column(CustomerReceives::PaymentMethodId)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(SalePayments::Table)
                            .drop_column(SalePayments::PaymentMethodId)
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::Accounting => {
                let tables: [DynIden; 5] = [
                    DepositWithdraws::Table.into_iden(),
                    Expenses::Table.into_iden(),
                    Incomes::Table.into_iden(),
                    ExpenseCategories::Table.into_iden(),
                    IncomeCategories::Table.into_iden(),
                ];
                for table in tables {
                    manager
                        .drop_table(Table::drop().table(table).if_exists().to_owned())
                        .await?;
                }
                let conn = manager.get_connection();
                for name in ACCOUNTING_PERMISSIONS {
                    let row = permissions::Entity::find()
                        .filter(permissions::Column::Name.eq(*name))
                        .one(conn)
                        .await?;
                    if let Some(row) = row {
                        permissions::Entity::delete_by_id(row.id).exec(conn).await?;
                    }
                }
            }
            Migrations::Purchases => {
                let tables: [DynIden; 5] = [
                    PurchaseReturnDetails::Table.into_iden(),
                    PurchaseReturns::Table.into_iden(),
                    PurchaseDetails::Table.into_iden(),
                    Purchases::Table.into_iden(),
                    PaymentMethods::Table.into_iden(),
                ];
                for table in tables {
                    manager
                        .drop_table(Table::drop().table(table).if_exists().to_owned())
                        .await?;
                }
                manager
                    .alter_table(
                        Table::alter()
                            .table(SupplierPayments::Table)
                            .drop_column(SupplierPayments::PurchaseId)
                            .to_owned(),
                    )
                    .await?;
                let conn = manager.get_connection();
                for name in PURCHASE_PERMISSIONS {
                    let row = permissions::Entity::find()
                        .filter(permissions::Column::Name.eq(*name))
                        .one(conn)
                        .await?;
                    if let Some(row) = row {
                        permissions::Entity::delete_by_id(row.id).exec(conn).await?;
                    }
                }
            }
            Migrations::LoyaltyPointsWidth => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(LoyaltyEntries::Table)
                            .modify_column(
                                ColumnDef::new(LoyaltyEntries::Points)
                                    .integer()
                                    .not_null(),
                            )
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::FixedAssets => {
                manager
                    .drop_table(
                        Table::drop()
                            .table(FixedAssetMovements::Table)
                            .if_exists()
                            .to_owned(),
                    )
                    .await?;
                manager
                    .drop_table(
                        Table::drop()
                            .table(FixedAssetItems::Table)
                            .if_exists()
                            .to_owned(),
                    )
                    .await?;
                let conn = manager.get_connection();
                for name in FIXED_ASSET_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::ItemBatches => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(StockMovements::Table)
                            .drop_column(StockMovements::BatchId)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .drop_table(
                        Table::drop().table(ItemBatches::Table).if_exists().to_owned(),
                    )
                    .await?;
            }
            Migrations::ItemVariationDepth => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Items::Table)
                            .drop_column(Items::ParentId)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(Items::Table)
                            .drop_column(Items::Symbology)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(Items::Table)
                            .drop_column(Items::Weighed)
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::ItemSubCategories => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Items::Table)
                            .drop_column(Items::SubCategoryId)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .drop_table(Table::drop().table(ItemSubCategories::Table).if_exists().to_owned())
                    .await?;
            }
            Migrations::ManagerApprovals => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Users::Table)
                            .drop_column(Users::PinHash)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(Sales::Table)
                            .drop_column(Sales::ApprovedBy)
                            .to_owned(),
                    )
                    .await?;
                manager
                    .alter_table(
                        Table::alter()
                            .table(SaleReturns::Table)
                            .drop_column(SaleReturns::ApprovedBy)
                            .to_owned(),
                    )
                    .await?;
                let conn = manager.get_connection();
                for name in APPROVAL_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::CreditNotes => {
                manager
                    .drop_table(Table::drop().table(CreditNotes::Table).if_exists().to_owned())
                    .await?;
                let conn = manager.get_connection();
                for name in CREDIT_NOTE_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::SaleRounding => {
                manager
                    .alter_table(
                        Table::alter()
                            .table(Sales::Table)
                            .drop_column(Sales::Rounding)
                            .to_owned(),
                    )
                    .await?;
            }
            Migrations::ServiceRatings => {
                manager
                    .drop_table(Table::drop().table(ServiceRatings::Table).if_exists().to_owned())
                    .await?;
                let conn = manager.get_connection();
                for name in RATING_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
            Migrations::Loyalty => {
                manager
                    .drop_table(Table::drop().table(LoyaltyEntries::Table).if_exists().to_owned())
                    .await?;
            }
            Migrations::GiftCards => {
                for t in [GiftCardTransactions::Table.into_iden(), GiftCards::Table.into_iden()] {
                    manager.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
                }
                let conn = manager.get_connection();
                for name in GIFT_CARD_PERMISSIONS {
                    permissions::Entity::delete_many()
                        .filter(permissions::Column::Name.eq(*name))
                        .exec(conn)
                        .await?;
                }
            }
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Users::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Users::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(Permissions::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Permissions::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(Roles::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Roles::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Units::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Units::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(Brands::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Brands::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(ItemCategories::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(ItemCategories::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Items::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Items::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Sales::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Sales::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(SaleDetails::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(StockMovements::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Customers::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Customers::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(Suppliers::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Suppliers::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(Registers::Table)
                .if_not_exists()
                .col(ColumnDef::new(Registers::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Registers::UserId).integer().not_null())
                // 'Open' or 'Closed'. Strings, like every other status here.
                .col(ColumnDef::new(Registers::Status).string().not_null())
                .col(ColumnDef::new(Registers::OpenedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Registers::ClosedAt).custom(ts).null())
                .col(ColumnDef::new(Registers::OpeningBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                // Per-method opening float as JSON, mirroring the reference's
                // `opening_details`: `[{"method":"Cash","amount":"50000.000"}]`.
                .col(ColumnDef::new(Registers::OpeningDetails).text().null())
                // What the cashier counted at close; expected is the snapshot below.
                .col(ColumnDef::new(Registers::ClosingBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Registers::ExpectedBalance).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).null())
                .col(ColumnDef::new(Registers::Note).string().null())
                .col(ColumnDef::new(Registers::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(SaleReturns::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Bookings::StartAt).custom(ts).not_null())
                .col(ColumnDef::new(Bookings::EndAt).custom(ts).not_null())
                .col(ColumnDef::new(Bookings::Note).string().null())
                .col(ColumnDef::new(Bookings::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Bookings::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Bookings::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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

/// Store credit against a customer, issued from a return. A return reverses one
/// sale; a credit note settles credit across several: the customer owes less on
/// *future* sales rather than getting cash back now.
///
/// The balance math already counts `customer_receives`, so a note that can be
/// spent must post into the same figure — it is recorded as a receipt in
/// the same table, with the note number as its reference. No new money table,
/// no second source for what was paid.
async fn credit_notes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(CreditNotes::Table)
                .if_not_exists()
                .col(ColumnDef::new(CreditNotes::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(CreditNotes::CreditNo).string().not_null().unique_key().to_owned())
                .col(ColumnDef::new(CreditNotes::CustomerId).integer().not_null())
                .col(ColumnDef::new(CreditNotes::SaleReturnId).integer().null())
                .col(ColumnDef::new(CreditNotes::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(CreditNotes::AppliedTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(CreditNotes::CreatedBy).integer().null())
                .col(ColumnDef::new(CreditNotes::Note).string().null())
                .col(ColumnDef::new(CreditNotes::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(CreditNotes::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_credit_notes_customer")
                        .from(CreditNotes::Table, CreditNotes::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_credit_notes_return")
                        .from(CreditNotes::Table, CreditNotes::SaleReturnId)
                        .to(SaleReturns::Table, SaleReturns::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_credit_notes_customer_id")
                .table(CreditNotes::Table)
                .col(CreditNotes::CustomerId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in CREDIT_NOTE_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("creditnote");
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

/// Cash round-off, posted on the sale — not into thin air. The reference carries
/// a `rounding` column but writes 0 unconditionally; here it holds
/// `rounded_total - grand_total` for cash sales, so `SUM(rounding)` over a period
/// is exactly what rounding gained or cost the till.
async fn sale_rounding(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Sales::Table)
                .add_column(
                    ColumnDef::new(Sales::Rounding)
                        .decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE)
                        .not_null()
                        .default(0)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// One-tap service ratings from the customer display. A `Like` or `Dislike` per
/// completed sale, anonymous and uneditable — the same screen is offered for
/// every rating, so there is no path that filters criticism out.
async fn service_ratings(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(ServiceRatings::Table)
                .if_not_exists()
                .col(ColumnDef::new(ServiceRatings::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ServiceRatings::SaleId).integer().null())
                .col(ColumnDef::new(ServiceRatings::Rating).string().not_null())
                .col(ColumnDef::new(ServiceRatings::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_service_ratings_sale")
                        .from(ServiceRatings::Table, ServiceRatings::SaleId)
                        .to(Sales::Table, Sales::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_service_ratings_sale_id")
                .table(ServiceRatings::Table)
                .col(ServiceRatings::SaleId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in RATING_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("rating");
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

/// Loyalty points as a ledger, like stock: immutable Earn/Redeem/Void rows, never
/// a mutated balance. One point per Rp1.000 of paid total (floored), redeemable
/// at Rp1 each — fixed until the settings table lands and can hold the rate.
///
/// This supersedes `customers.loyalty_points`, which stops being written: a
/// stored balance is a read-modify-write that two concurrent sales interleave,
/// the same reason stock and money are ledgers here. The column stays (dropping
/// it strands old databases); the views derive from this table instead.
async fn loyalty(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(LoyaltyEntries::Table)
                .if_not_exists()
                .col(ColumnDef::new(LoyaltyEntries::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(LoyaltyEntries::CustomerId).integer().not_null())
                .col(ColumnDef::new(LoyaltyEntries::SaleId).integer().null())
                // `Earn`, `Redeem` or `Void`. Signed `points`: earn is positive.
                .col(ColumnDef::new(LoyaltyEntries::Kind).string().not_null())
                .col(ColumnDef::new(LoyaltyEntries::Points).integer().not_null())
                .col(ColumnDef::new(LoyaltyEntries::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_loyalty_entries_customer")
                        .from(LoyaltyEntries::Table, LoyaltyEntries::CustomerId)
                        .to(Customers::Table, Customers::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_loyalty_entries_sale")
                        .from(LoyaltyEntries::Table, LoyaltyEntries::SaleId)
                        .to(Sales::Table, Sales::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_loyalty_entries_customer_id")
                .table(LoyaltyEntries::Table)
                .col(LoyaltyEntries::CustomerId)
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Second pair of eyes for risky till actions. The reference carries a
/// `discount_permission_code` per user but never checks it at the till; here the
/// manager's PIN is verified server-side before a discount, return, or price
/// override commits, and the approver's id is stored on the row.
///
/// `pin_hash` mirrors `password_hash` (Argon2 PHC string, never on the wire),
/// because a 4-digit secret stored in cleartext is a gift to anyone who reads
/// the database file. The `approved_by` columns record who authorised a risky
/// row; plain integers, no FK, so the audit trail outlives account hygiene.
async fn manager_approvals(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Users::Table)
                .add_column(ColumnDef::new(Users::PinHash).string().null().to_owned())
                .to_owned(),
        )
        .await?;

    // Who approved the risky row. Plain integer, no FK — same as `created_by` on
    // the installment and credit-note tables: the audit trail outlives account
    // hygiene, and a hard link would force a choice between deleting history or
    // keeping dead accounts.
    manager
        .alter_table(
            Table::alter()
                .table(Sales::Table)
                .add_column(ColumnDef::new(Sales::ApprovedBy).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(SaleReturns::Table)
                .add_column(ColumnDef::new(SaleReturns::ApprovedBy).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in APPROVAL_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("approval");
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

/// Second-level catalog grouping: a category's children. The reference's
/// `ItemSubCategory` belongs to one `ItemCategory` and owns items through
/// `sub_category_id`; deleting a category cascades in the reference, but here
/// both links are `SetNull` like every other catalog FK — deleting a grouping
/// must not delete the items in it.
async fn item_sub_categories(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(ItemSubCategories::Table)
                .if_not_exists()
                .col(ColumnDef::new(ItemSubCategories::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ItemSubCategories::CategoryId).integer().not_null())
                .col(ColumnDef::new(ItemSubCategories::Name).string().not_null())
                .col(ColumnDef::new(ItemSubCategories::Description).string().null())
                .col(ColumnDef::new(ItemSubCategories::SortId).integer().not_null().default(0))
                .col(ColumnDef::new(ItemSubCategories::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(ItemSubCategories::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(ItemSubCategories::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(&mut ForeignKey::create().name("fk_item_sub_categories_category").from(ItemSubCategories::Table, ItemSubCategories::CategoryId).to(ItemCategories::Table, ItemCategories::Id).on_delete(ForeignKeyAction::Cascade).to_owned())
                .to_owned(),
        )
        .await?;

    // Items point at their sub-category; the column lands here rather than in the
    // original `items` migration so existing databases get it too.
    manager
        .alter_table(
            Table::alter()
                .table(Items::Table)
                .add_column(ColumnDef::new(Items::SubCategoryId).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    // Foreign keys cannot be added to an existing table on SQLite — sea-query
    // panics — so only Postgres gets the constraint. The application always
    // writes a valid id or NULL either way.
    if manager.get_database_backend() != sea_orm::DbBackend::Sqlite {
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_items_sub_category")
                    .from(Items::Table, Items::SubCategoryId)
                    .to(ItemSubCategories::Table, ItemSubCategories::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;
    }

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_item_sub_categories_sort")
                .table(ItemSubCategories::Table)
                .col(ItemSubCategories::SortId)
                .to_owned(),
        )
        .await?;

    Ok(())
}

/// Variation depth on the item itself. The reference's variations are child
/// items (`parent_id` self-link) carrying a `variation_details` JSON blob —
/// size, colour, flavour — not a separate `variations` table; a T-shirt's
/// sizes are rows, and each holds its own stock. Same here: `parent_id` names
/// the template, NULL means standalone. `symbology` names how `code` scans —
/// a `0`-prefixed EAN-8 is not an internal SKU, and mixing them makes dedupe
/// wrong. `weighed` marks price-per-kg goods whose quantity comes from a
/// scale; checkout still takes the number, the scale half is Stage 11 USB.
async fn item_variation_depth(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Items::Table)
                .add_column(ColumnDef::new(Items::ParentId).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(Items::Table)
                .add_column(ColumnDef::new(Items::Symbology).string().null().to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(Items::Table)
                .add_column(ColumnDef::new(Items::Weighed).boolean().not_null().default(false).to_owned())
                .to_owned(),
        )
        .await?;

    // Same SQLite rule as the sub-category link: no FK on an existing table,
    // so only Postgres gets the constraint. The guard below enforces it anyway.
    if manager.get_database_backend() != sea_orm::DbBackend::Sqlite {
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_items_parent")
                    .from(Items::Table, Items::ParentId)
                    .to(Items::Table, Items::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;
    }

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_items_parent")
                .table(Items::Table)
                .col(Items::ParentId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_items_code")
                .table(Items::Table)
                .col(Items::Code)
                .to_owned(),
        )
        .await?;

    Ok(())
}

/// Per-lot dates for perishables and medicine. The reference stores an expiry
/// string per line description with no quantity split, so two lots of the same
/// product cannot be told apart at the till — FEFO needs to know which lot a
/// unit came from, and an expired lot must refuse to sell rather than sitting in
/// the same pile as a fresh one.
///
/// `batch_id` on the ledger is the whole design: on-hand per lot is `SUM(quantity)`
/// over the rows naming that lot, exactly as item-level on-hand already is. There
/// is deliberately no quantity column on `item_batches` for the same reason there
/// is none on `items` — a second figure that can disagree with the ledger is a
/// bug waiting to happen.
async fn item_batches(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(ItemBatches::Table)
                .if_not_exists()
                .col(ColumnDef::new(ItemBatches::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ItemBatches::ItemId).integer().not_null())
                .col(ColumnDef::new(ItemBatches::BatchNo).string().not_null())
                // A date, not a timestamp: an expiry is a calendar day, and a time
                // component only invites timezone arguments at the till.
                .col(ColumnDef::new(ItemBatches::ExpiryDate).date().null())
                .col(ColumnDef::new(ItemBatches::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(ItemBatches::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(ItemBatches::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_item_batches_item")
                        .from(ItemBatches::Table, ItemBatches::ItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(StockMovements::Table)
                .add_column(ColumnDef::new(StockMovements::BatchId).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    // Same SQLite limitation as every other added-column FK: it cannot be added
    // after the fact, so only Postgres gets the constraint. The application
    // always writes a live batch id or NULL.
    if manager.get_database_backend() != sea_orm::DbBackend::Sqlite {
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_stock_movements_batch")
                    .from(StockMovements::Table, StockMovements::BatchId)
                    .to(ItemBatches::Table, ItemBatches::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;
    }

    // FEFO reads batches for one item ordered by expiry, and the ledger read is
    // filtered per lot — both are index lookups rather than scans.
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_item_batches_item")
                .table(ItemBatches::Table)
                .col(ItemBatches::ItemId)
                .to_owned(),
        )
        .await?;

    // Unique per item rather than globally: two products from the same
    // manufacturer print the same lot number, and that is not a clash. A
    // composite key, so it is an index rather than a column's `unique_key`.
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .unique()
                .name("idx_item_batches_item_no")
                .table(ItemBatches::Table)
                .col(ItemBatches::ItemId)
                .col(ItemBatches::BatchNo)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_stock_movements_batch")
                .table(StockMovements::Table)
                .col(StockMovements::BatchId)
                .to_owned(),
        )
        .await?;

    Ok(())
}

/// Fixed assets — the fridge, the forklift, the till itself. A shop tracks these,
/// but they are **not sellable stock**: they never reach the shelf, never decrement
/// on a sale, and their quantity is not a fraction. So this is a separate catalog
/// from `items`, not a flag on it.
///
/// The reference splits this across four tables (`fixed_asset_stock_ins` plus
/// `_details`, and the matching `_outs`). Four tables where a signed ledger does the
/// same job: `fixed_asset_movements` records an asset arriving or leaving with a
/// quantity and the price it went at, and on-hand is `SUM(quantity)` over them — the
/// same derivation as stock, so the two subsystems agree on what a ledger is.
/// Widens `loyalty_entries.points` from 4 to 8 bytes.
///
/// The entity declares `i64`, which sqlx reads as `INT8` on Postgres — but the
/// original migration created the column with `.integer()`, which is `INT4` there.
/// Every read of that column then failed to decode. SQLite hid it: its `INTEGER`
/// is 64-bit regardless of how the column was declared, so the mismatch cannot
/// exist on that backend.
///
/// A data migration rather than an edit to `loyalty()`, because an existing
/// database has already applied that variant and would otherwise keep the narrow
/// column forever.
async fn loyalty_points_width(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // SQLite is a genuine no-op here rather than a skipped step: its `INTEGER` is a
    // signed 64-bit type whatever the declaration says, so the column already holds
    // the full `i64` range and there is nothing to widen. sea-query cannot express
    // the `ALTER` at all on that backend — it panics with `Sqlite not support
    // modifying table column`, which fails every test in the suite rather than one.
    if manager.get_database_backend() == DbBackend::Sqlite {
        return Ok(());
    }

    manager
        .alter_table(
            Table::alter()
                .table(LoyaltyEntries::Table)
                .modify_column(
                    ColumnDef::new(LoyaltyEntries::Points)
                        .big_integer()
                        .not_null(),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

/// Goods received from a supplier.
///
/// Three decisions here are departures from the reference, each because the
/// reference keeps a second source for a figure that is derivable:
///
/// - **No `paid` / `due_amount` column.** The reference stores both *and* has a
///   `purchase_payments` table *and* has `supplier_payments`, so three records of
///   what was paid can disagree. Here a payment is a `supplier_payments` row that
///   optionally names the purchase it settles, and what a purchase owes is
///   `grand_total - SUM` over those rows.
/// - **The reference writes no stock movement when a purchase is recorded.** It
///   has no ledger at all, so goods received through a purchase never reach its
///   stock views. Each line here writes a `GoodsReceipt` row, because on-hand is
///   `SUM(quantity)` over `stock_movements` and a purchase that moves nothing is a
///   purchase that never reaches the shelf.
/// - **The reference stores `discount` as a string** to preserve a trailing `%`,
///   which makes one column mean either a percentage or an amount depending on
///   what was typed. The input still accepts both, but the column holds the
///   resolved amount and `grand_total = subtotal - discount`.
///
/// `reference_no` is derived from the primary key at write time rather than
/// `MAX(reference_no) + 1`: two concurrent purchases read the same MAX.
async fn purchases(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(PaymentMethods::Table)
                .if_not_exists()
                .col(ColumnDef::new(PaymentMethods::Id).integer().not_null().auto_increment().primary_key().to_owned())
                // Not unique: "Cash" and "cash" are the same tender, and a
                // duplicate guard on the name would need to be case-insensitive
                // to be worth anything.
                .col(ColumnDef::new(PaymentMethods::Name).string().not_null())
                // `Cash` moves through the drawer; `Card`, `Qris`, `Transfer` do not.
                // Register close needs the split to count what is in the till.
                .col(ColumnDef::new(PaymentMethods::Kind).string().not_null())
                .col(ColumnDef::new(PaymentMethods::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(PaymentMethods::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Purchases::Table)
                .if_not_exists()
                .col(ColumnDef::new(Purchases::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Purchases::ReferenceNo).string().not_null())
                .col(ColumnDef::new(Purchases::SupplierId).integer().not_null())
                // The supplier's own invoice number, which is what an operator
                // reconciles against. Null is normal: not every supplier sends one.
                .col(ColumnDef::new(Purchases::SupplierInvoiceNo).string().null())
                .col(ColumnDef::new(Purchases::PurchasedAt).date().not_null())
                .col(ColumnDef::new(Purchases::Subtotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Purchases::Discount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Purchases::GrandTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Purchases::Note).string().null())
                .col(ColumnDef::new(Purchases::CreatedBy).integer().null())
                .col(ColumnDef::new(Purchases::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Purchases::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Purchases::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                // Restrict, not Cascade: a supplier that has been paid cannot be
                // deleted out from under the purchase that created the debt.
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchases_supplier")
                        .from(Purchases::Table, Purchases::SupplierId)
                        .to(Suppliers::Table, Suppliers::Id)
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
                .name("idx_purchases_supplier_id")
                .table(Purchases::Table)
                .col(Purchases::SupplierId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(PurchaseDetails::Table)
                .if_not_exists()
                .col(ColumnDef::new(PurchaseDetails::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(PurchaseDetails::PurchaseId).integer().not_null())
                .col(ColumnDef::new(PurchaseDetails::ItemId).integer().not_null())
                // Null for goods with no expiry, which is most of a shop.
                .col(ColumnDef::new(PurchaseDetails::BatchId).integer().null())
                .col(ColumnDef::new(PurchaseDetails::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(PurchaseDetails::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(PurchaseDetails::Total).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_details_purchase")
                        .from(PurchaseDetails::Table, PurchaseDetails::PurchaseId)
                        .to(Purchases::Table, Purchases::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_details_item")
                        .from(PurchaseDetails::Table, PurchaseDetails::ItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_details_batch")
                        .from(PurchaseDetails::Table, PurchaseDetails::BatchId)
                        .to(ItemBatches::Table, ItemBatches::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_purchase_details_purchase_id")
                .table(PurchaseDetails::Table)
                .col(PurchaseDetails::PurchaseId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(PurchaseReturns::Table)
                .if_not_exists()
                .col(ColumnDef::new(PurchaseReturns::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(PurchaseReturns::ReferenceNo).string().not_null())
                // Which purchase is being corrected. A return without one is not
                // supported: a supplier sends goods back against an invoice.
                .col(ColumnDef::new(PurchaseReturns::PurchaseId).integer().not_null())
                // Denormalized from the purchase so a returns list does not need a
                // join to name the supplier, and so the supplier still reads
                // correctly if the purchase is ever corrected.
                .col(ColumnDef::new(PurchaseReturns::SupplierId).integer().not_null())
                .col(ColumnDef::new(PurchaseReturns::ReturnedAt).date().not_null())
                .col(ColumnDef::new(PurchaseReturns::TotalAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(PurchaseReturns::Note).string().null())
                .col(ColumnDef::new(PurchaseReturns::CreatedBy).integer().null())
                .col(ColumnDef::new(PurchaseReturns::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(PurchaseReturns::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(PurchaseReturns::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_returns_purchase")
                        .from(PurchaseReturns::Table, PurchaseReturns::PurchaseId)
                        .to(Purchases::Table, Purchases::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_returns_supplier")
                        .from(PurchaseReturns::Table, PurchaseReturns::SupplierId)
                        .to(Suppliers::Table, Suppliers::Id)
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
                .name("idx_purchase_returns_purchase_id")
                .table(PurchaseReturns::Table)
                .col(PurchaseReturns::PurchaseId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(PurchaseReturnDetails::Table)
                .if_not_exists()
                .col(ColumnDef::new(PurchaseReturnDetails::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(PurchaseReturnDetails::PurchaseReturnId).integer().not_null())
                // Which purchase line is being corrected, rather than just the item.
                // A shop that buys the same item twice has two lots of it on the
                // shelf, and a return naming only the item could draw down the
                // wrong one.
                .col(ColumnDef::new(PurchaseReturnDetails::PurchaseDetailId).integer().not_null())
                .col(ColumnDef::new(PurchaseReturnDetails::ItemId).integer().not_null())
                .col(ColumnDef::new(PurchaseReturnDetails::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(PurchaseReturnDetails::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(PurchaseReturnDetails::Total).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_return_details_return")
                        .from(PurchaseReturnDetails::Table, PurchaseReturnDetails::PurchaseReturnId)
                        .to(PurchaseReturns::Table, PurchaseReturns::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_return_details_item")
                        .from(PurchaseReturnDetails::Table, PurchaseReturnDetails::ItemId)
                        .to(Items::Table, Items::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_purchase_return_details_line")
                        .from(PurchaseReturnDetails::Table, PurchaseReturnDetails::PurchaseDetailId)
                        .to(PurchaseDetails::Table, PurchaseDetails::Id)
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
                .name("idx_purchase_return_details_return_id")
                .table(PurchaseReturnDetails::Table)
                .col(PurchaseReturnDetails::PurchaseReturnId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_purchase_return_details_line_id")
                .table(PurchaseReturnDetails::Table)
                .col(PurchaseReturnDetails::PurchaseDetailId)
                .to_owned(),
        )
        .await?;

    // One payment table, not two. The reference has `purchase_payments` and
    // `supplier_payments` covering the same money; Stage 3 already built
    // `supplier_payments`, so a purchase is settled by pointing a payment at it.
    // SetNull rather than Cascade: a payment is money that left, and losing it
    // would quietly change a supplier's balance.
    // One statement per change. SQLite cannot apply several alter options in a
    // single `ALTER` — sea-query panics with `Sqlite doesn't support multiple
    // alter options` — so a combined add-column-and-constraint statement fails
    // every test in the suite rather than one.
    manager
        .alter_table(
            Table::alter()
                .table(SupplierPayments::Table)
                .add_column(
                    ColumnDef::new(SupplierPayments::PurchaseId)
                        .integer()
                        .null()
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(SupplierPayments::Table)
                .add_column(
                    ColumnDef::new(SupplierPayments::PaymentMethodId)
                        .integer()
                        .null()
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    // SQLite cannot add a constraint to a table that already exists, so the two
    // links are declared on the backends that can. Neither is load-bearing where
    // it is missing: a purchase is immutable and undeletable by command, and a
    // payment method is soft-deleted rather than removed — so neither target can
    // disappear out from under a payment.
    if manager.get_database_backend() != DbBackend::Sqlite {
        manager
            .alter_table(
                Table::alter()
                    .table(SupplierPayments::Table)
                    .add_foreign_key(
                        &mut TableForeignKey::new()
                            .name("fk_supplier_payments_purchase")
                            .from_tbl(SupplierPayments::Table)
                            .from_col(SupplierPayments::PurchaseId)
                            .to_tbl(Purchases::Table)
                            .to_col(Purchases::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(SupplierPayments::Table)
                    .add_foreign_key(
                        &mut TableForeignKey::new()
                            .name("fk_supplier_payments_method")
                            .from_tbl(SupplierPayments::Table)
                            .from_col(SupplierPayments::PaymentMethodId)
                            .to_tbl(PaymentMethods::Table)
                            .to_col(PaymentMethods::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
    }

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_supplier_payments_purchase_id")
                .table(SupplierPayments::Table)
                .col(SupplierPayments::PurchaseId)
                .to_owned(),
        )
        .await?;

    // The tenders a shop actually uses, seeded because a fresh install cannot
    // record a payment until one exists, and the reference seeds them too. An
    // operator adds more from Settings.
    let conn = manager.get_connection();
    let now = now();
    for (name, kind) in [
        ("Cash", "Cash"),
        ("Bank Transfer", "Transfer"),
        ("Debit/Credit Card", "Card"),
        ("QRIS", "Qris"),
        ("E-Wallet", "EWallet"),
        ("Credit", "Credit"),
    ] {
        use sea_orm::entity::prelude::EntityTrait;

        let exists = crate::entities::trade::payment_method::Entity::find()
            .filter(crate::entities::trade::payment_method::Column::Name.eq(name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        crate::entities::trade::payment_method::ActiveModel {
            name: Set(name.to_owned()),
            kind: Set(kind.to_owned()),
            del_status: Set(DEL_LIVE.to_owned()),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(conn)
        .await?;
    }

    for name in PURCHASE_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("purchase");
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

/// Stage 8 — the people who work here.
///
/// An employee is not an account. A counter hand, a part-timer, a family member: they are
/// on the roster, they are in the salary run, they answer for their attendance, and they
/// have no login, no password and no permissions. Putting them in `users` means either a
/// password they never use or a payroll entry for an account that was never a person.
///
/// `user_id` links the two where they overlap — a manager who is both — and is null
/// otherwise, which is the common case and not an incomplete record.
///
/// `base_salary` lives here rather than on the salary run, because a salary run that
/// makes the operator re-key everyone's monthly rate every month is not a salary module.
///
/// This runs *before* `accounting` on purpose: `incomes.employee_id` and
/// `expenses.employee_id` were written against `users` as placeholders, and both were
/// named "Stage 8's employee records supersede it". A foreign key cannot be re-pointed
/// afterwards — SQLite cannot alter a constraint at all, and sea-query panics rather than
/// emitting broken DDL — so the table has to exist first. Neither accounting migration has
/// run anywhere, which is what makes correcting them in place the honest move instead of a
/// repair migration for an install that does not exist.
async fn employees(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(Employees::Table)
                .if_not_exists()
                .col(ColumnDef::new(Employees::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Employees::Name).string().not_null())
                .col(ColumnDef::new(Employees::Phone).string().null())
                .col(ColumnDef::new(Employees::Email).string().null())
                .col(ColumnDef::new(Employees::Address).string().null())
                .col(ColumnDef::new(Employees::UserId).integer().null())
                .col(ColumnDef::new(Employees::BaseSalary).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Employees::HireDate).date().null())
                .col(ColumnDef::new(Employees::TerminatedOn).date().null())
                .col(ColumnDef::new(Employees::Note).string().null())
                .col(ColumnDef::new(Employees::CreatedBy).integer().null())
                .col(ColumnDef::new(Employees::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Employees::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Employees::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_employees_user")
                        .from(Employees::Table, Employees::UserId)
                        .to(Users::Table, Users::Id)
                        // SetNull, matching every other soft-deletable link: an account
                        // going away must not delete the person, because their salary
                        // history still has to balance.
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    // A non-unique index cannot be inlined into CREATE TABLE — it renders as
    // `CONSTRAINT "..." ("col")`, which is invalid on both Postgres and SQLite. Only
    // `unique_key()` is legal inline.
    for (name, col) in [("idx_employees_name", Employees::Name)] {
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name(name)
                    .table(Employees::Table)
                    .col(col)
                    .to_owned(),
            )
            .await?;
    }

    Ok(())
}

/// Stage 7 — the cash book: income, expense, and the owner's deposits and withdrawals.
///
/// Three tables rather than one with a `direction` column, because the three answer
/// different questions. **Income** is the shop earning outside trading, **expense** is
/// it spending outside purchases, and a **deposit/withdrawal** is the owner moving
/// money into or out of a tender — which changes what the drawer holds without
/// changing what the shop earned. One table with a sign would make a float top-up read
/// as revenue and every profit figure built on it wrong.
///
/// `payment_methods` is not created here: Stage 6 made it, and a tender is the same
/// vocabulary whichever of these is posted against it.
///
/// **Nothing here stores a balance.** What a tender holds is the signed sum over these
/// rows, derived — a stored balance is a read-modify-write, the same reason stock is a
/// ledger and customer balances are derived.
async fn accounting(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    // `name` is not unique, matching units, brands and categories: "Rent" and "rent"
    // are the same expense and a case-sensitive constraint would refuse one of them.
    manager
        .create_table(
            Table::create()
                .table(IncomeCategories::Table)
                .if_not_exists()
                .col(ColumnDef::new(IncomeCategories::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(IncomeCategories::Name).string().not_null())
                .col(ColumnDef::new(IncomeCategories::Description).string().null())
                .col(ColumnDef::new(IncomeCategories::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(IncomeCategories::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ExpenseCategories::Table)
                .if_not_exists()
                .col(ColumnDef::new(ExpenseCategories::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ExpenseCategories::Name).string().not_null())
                .col(ColumnDef::new(ExpenseCategories::Description).string().null())
                .col(ColumnDef::new(ExpenseCategories::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(ExpenseCategories::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Incomes::Table)
                .if_not_exists()
                .col(ColumnDef::new(Incomes::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Incomes::ReferenceNo).string().not_null())
                .col(ColumnDef::new(Incomes::IncomeCategoryId).integer().not_null())
                .col(ColumnDef::new(Incomes::PaymentMethodId).integer().not_null())
                // Who the money belongs to when that is not the account posting it.
                .col(ColumnDef::new(Incomes::EmployeeId).integer().null())
                .col(ColumnDef::new(Incomes::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(Incomes::OccurredAt).date().not_null())
                .col(ColumnDef::new(Incomes::Note).string().null())
                .col(ColumnDef::new(Incomes::CreatedBy).integer().null())
                .col(ColumnDef::new(Incomes::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Incomes::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Incomes::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                // Restrict on both targets: a category that has been posted against is
                // part of a posted cash-book line, and a tender is the thing the
                // balance is grouped by.
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_incomes_category")
                        .from(Incomes::Table, Incomes::IncomeCategoryId)
                        .to(IncomeCategories::Table, IncomeCategories::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_incomes_payment_method")
                        .from(Incomes::Table, Incomes::PaymentMethodId)
                        .to(PaymentMethods::Table, PaymentMethods::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                // SetNull rather than Cascade: a posted line does not stop being true
                // because the person it named left. Deleting an account is a
                // soft-delete, so this only fires on a hard delete.
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_incomes_employee")
                        .from(Incomes::Table, Incomes::EmployeeId)
                        .to(Employees::Table, Employees::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_incomes_category_id")
                .table(Incomes::Table)
                .col(Incomes::IncomeCategoryId)
                .to_owned(),
        )
        .await?;

    // The cash book is read per tender, so that is the index an account statement
    // actually uses.
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_incomes_payment_method_id")
                .table(Incomes::Table)
                .col(Incomes::PaymentMethodId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Expenses::Table)
                .if_not_exists()
                .col(ColumnDef::new(Expenses::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(Expenses::ReferenceNo).string().not_null())
                .col(ColumnDef::new(Expenses::ExpenseCategoryId).integer().not_null())
                .col(ColumnDef::new(Expenses::PaymentMethodId).integer().not_null())
                .col(ColumnDef::new(Expenses::EmployeeId).integer().null())
                .col(ColumnDef::new(Expenses::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(Expenses::OccurredAt).date().not_null())
                .col(ColumnDef::new(Expenses::Note).string().null())
                .col(ColumnDef::new(Expenses::CreatedBy).integer().null())
                .col(ColumnDef::new(Expenses::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Expenses::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Expenses::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_expenses_category")
                        .from(Expenses::Table, Expenses::ExpenseCategoryId)
                        .to(ExpenseCategories::Table, ExpenseCategories::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_expenses_payment_method")
                        .from(Expenses::Table, Expenses::PaymentMethodId)
                        .to(PaymentMethods::Table, PaymentMethods::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_expenses_employee")
                        .from(Expenses::Table, Expenses::EmployeeId)
                        .to(Employees::Table, Employees::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_expenses_category_id")
                .table(Expenses::Table)
                .col(Expenses::ExpenseCategoryId)
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_expenses_payment_method_id")
                .table(Expenses::Table)
                .col(Expenses::PaymentMethodId)
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(DepositWithdraws::Table)
                .if_not_exists()
                .col(ColumnDef::new(DepositWithdraws::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(DepositWithdraws::ReferenceNo).string().not_null())
                // Not optional: the whole point of the row is that *this* tender moved.
                .col(ColumnDef::new(DepositWithdraws::PaymentMethodId).integer().not_null())
                // `Deposit` or `Withdraw`, so a report groups without matching strings.
                .col(ColumnDef::new(DepositWithdraws::Kind).string().not_null())
                .col(ColumnDef::new(DepositWithdraws::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(DepositWithdraws::OccurredAt).date().not_null())
                .col(ColumnDef::new(DepositWithdraws::Note).string().null())
                .col(ColumnDef::new(DepositWithdraws::CreatedBy).integer().null())
                .col(ColumnDef::new(DepositWithdraws::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(DepositWithdraws::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(DepositWithdraws::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_deposit_withdraws_payment_method")
                        .from(DepositWithdraws::Table, DepositWithdraws::PaymentMethodId)
                        .to(PaymentMethods::Table, PaymentMethods::Id)
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
                .name("idx_deposit_withdraws_payment_method_id")
                .table(DepositWithdraws::Table)
                .col(DepositWithdraws::PaymentMethodId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in ACCOUNTING_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("accounting");
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

/// Give the other two ways money enters the shop the tender Stage 6 gave the third.
///
/// `supplier_payments` already names a `payment_methods` row. `sale_payments` held
/// whatever the operator typed at the till and `customer_receives` held no tender at
/// all — three schemas for one concept, which is how a cash book ends up unable to
/// answer "how much is in the drawer".
///
/// `sale_payments.method` is **kept**. It is what the till displayed and what an old
/// receipt names, and the new column is nullable precisely so an existing row whose
/// text matches no live tender survives with its text intact rather than being
/// rewritten or dropped. The text stops being the source; the id is.
///
/// `customer_receives` is **not** backfilled, because nothing recorded the tender to
/// backfill from. A historical receipt's method is unknown, and null says that while a
/// guess would not.
async fn tender_references(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    // One statement per change: SQLite cannot apply several alter options in a
    // single `ALTER` — sea-query panics with `Sqlite doesn't support multiple alter
    // options` — so a combined statement would fail every test in the suite.
    manager
        .alter_table(
            Table::alter()
                .table(SalePayments::Table)
                .add_column(ColumnDef::new(SalePayments::PaymentMethodId).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    manager
        .alter_table(
            Table::alter()
                .table(CustomerReceives::Table)
                .add_column(
                    ColumnDef::new(CustomerReceives::PaymentMethodId)
                        .integer()
                        .null()
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    // A single-tender sale writes no tender rows — the header carries the method and
    // the figure, and register close falls back to the header for exactly that case.
    // The header therefore needs the account too, or those sales are the ones the
    // cash book cannot see.
    manager
        .alter_table(
            Table::alter()
                .table(Sales::Table)
                .add_column(ColumnDef::new(Sales::PaymentMethodId).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    // The cash book reads per tender, so that is the index an account statement uses.
    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_sale_payments_payment_method_id")
                .table(SalePayments::Table)
                .col(SalePayments::PaymentMethodId)
                .to_owned(),
        )
        .await?;

    // Match case-insensitively, because the text came from a keyboard and "cash" and
    // "Cash" are the same tender. First live match wins, so a method duplicated
    // across spellings resolves to one row rather than arbitrarily.
    let conn = manager.get_connection();
    let methods = payment_method::Entity::find()
        .filter(payment_method::Column::DelStatus.eq(DEL_LIVE))
        .all(conn)
        .await?;

    let resolve = |text: &str| -> Option<i32> {
        let wanted = text.trim().to_lowercase();
        methods
            .iter()
            .find(|m| m.name.trim().to_lowercase() == wanted)
            .map(|m| m.id)
    };

    for row in sale_payment::Entity::find().all(conn).await? {
        let Some(id) = resolve(&row.method) else { continue };
        let mut am: sale_payment::ActiveModel = row.into();
        am.payment_method_id = Set(Some(id));
        am.update(conn).await?;
    }

    for row in sale::Entity::find().all(conn).await? {
        // A split sale's header reads "Cash + Debit/Credit Card", which matches no
        // one account — correctly so, because the accounts are on its tender rows.
        let Some(id) = resolve(&row.payment_method) else { continue };
        let mut am: sale::ActiveModel = row.into();
        am.payment_method_id = Set(Some(id));
        am.update(conn).await?;
    }

    Ok(())
}

/// A schedule that proposes recurring expenses, and the back-link from a posted expense.
///
/// **The schedule proposes; the expense posts.** Nothing here writes to the cash book
/// directly — posting one writes a real `expenses` row naming this, so the cash book
/// needs no special case and a posted expense is financial history like any other.
///
/// The reference has `payment_method` as free text here too, and `day_of_month`,
/// `day_of_week` and `billing_cycle` to describe the same schedule three ways. This
/// keeps the account as a reference and lets the rotation arithmetic derive the date,
/// because a column that must agree with three others is three places to be wrong.
async fn recurring_expenses(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(ExpenseRecurrings::Table)
                .if_not_exists()
                .col(ColumnDef::new(ExpenseRecurrings::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(ExpenseRecurrings::ExpenseCategoryId).integer().not_null())
                .col(ColumnDef::new(ExpenseRecurrings::Name).string().not_null())
                .col(ColumnDef::new(ExpenseRecurrings::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(ExpenseRecurrings::PaymentMethodId).integer().not_null())
                .col(ColumnDef::new(ExpenseRecurrings::Rotation).string().not_null())
                .col(ColumnDef::new(ExpenseRecurrings::StartsOn).date().not_null())
                // Always set while live, so null means exactly one thing.
                .col(ColumnDef::new(ExpenseRecurrings::NextDueOn).date().null())
                // Null is open-ended: rent has no last month.
                .col(ColumnDef::new(ExpenseRecurrings::EndsOn).date().null())
                .col(ColumnDef::new(ExpenseRecurrings::Note).string().null())
                .col(ColumnDef::new(ExpenseRecurrings::CreatedBy).integer().null())
                .col(ColumnDef::new(ExpenseRecurrings::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(ExpenseRecurrings::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(ExpenseRecurrings::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                // Restrict on the category, like an expense itself: a schedule with a
                // category that can be deleted would post into nothing.
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_expense_recurrings_category")
                        .from(ExpenseRecurrings::Table, ExpenseRecurrings::ExpenseCategoryId)
                        .to(ExpenseCategories::Table, ExpenseCategories::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_expense_recurrings_payment_method")
                        .from(ExpenseRecurrings::Table, ExpenseRecurrings::PaymentMethodId)
                        .to(PaymentMethods::Table, PaymentMethods::Id)
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
                .name("idx_expense_recurrings_next_due_on")
                .table(ExpenseRecurrings::Table)
                .col(ExpenseRecurrings::NextDueOn)
                .to_owned(),
        )
        .await?;

    // SetNull, not Cascade: stopping a schedule must not take posted history with it,
    // and a soft delete is what stopping a schedule does.
    manager
        .alter_table(
            Table::alter()
                .table(Expenses::Table)
                .add_column(ColumnDef::new(Expenses::RecurringExpenseId).integer().null().to_owned())
                .to_owned(),
        )
        .await?;

    if manager.get_database_backend() != DbBackend::Sqlite {
        manager
            .alter_table(
                Table::alter()
                    .table(Expenses::Table)
                    .add_foreign_key(
                        &mut TableForeignKey::new()
                            .name("fk_expenses_recurring")
                            .from_tbl(Expenses::Table)
                            .from_col(Expenses::RecurringExpenseId)
                            .to_tbl(ExpenseRecurrings::Table)
                            .to_col(ExpenseRecurrings::Id)
                            .on_delete(ForeignKeyAction::SetNull)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
    }

    let conn = manager.get_connection();
    let now = now();
    for name in RECURRING_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        permissions::ActiveModel {
            name: Set((*name).to_owned()),
            // Derived from the name like every other group, rather than written out.
            // Hardcoding `"recurring-expense"` here made this the one seeder whose
            // `group_name` disagreed with its own prefix, which is the second source
            // the catalog exists to prevent.
            group_name: Set(name.split_once('-').map(|(g, _)| g).unwrap_or("recurring").to_owned()),
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

async fn fixed_assets(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(FixedAssetItems::Table)
                .if_not_exists()
                .col(ColumnDef::new(FixedAssetItems::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(FixedAssetItems::Name).string().not_null())
                .col(ColumnDef::new(FixedAssetItems::Code).string().not_null().unique_key())
                .col(ColumnDef::new(FixedAssetItems::Description).string().null())
                .col(ColumnDef::new(FixedAssetItems::PurchasePrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(FixedAssetItems::SalePrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(FixedAssetItems::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(FixedAssetItems::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(FixedAssetItems::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(FixedAssetMovements::Table)
                .if_not_exists()
                .col(ColumnDef::new(FixedAssetMovements::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(FixedAssetMovements::AssetItemId).integer().not_null())
                // `In` when the asset arrives (bought, donated, found) and `Out`
                // when it leaves (sold, written off, scrapped).
                .col(ColumnDef::new(FixedAssetMovements::MovementKind).string().not_null())
                .col(ColumnDef::new(FixedAssetMovements::Quantity).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                // What it went at, kept per movement rather than on the item: the
                // same fridge can be bought at one price and sold at another, and
                // a valuation wants the purchase figures.
                .col(ColumnDef::new(FixedAssetMovements::UnitPrice).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(FixedAssetMovements::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(FixedAssetMovements::ReferenceNo).string().null())
                .col(ColumnDef::new(FixedAssetMovements::Note).string().null())
                .col(ColumnDef::new(FixedAssetMovements::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                // CASCADE: a movement with no asset is a line with no meaning, and an
                // asset's history is not a financial record the way a sale is.
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_fixed_asset_movements_item")
                        .from(FixedAssetMovements::Table, FixedAssetMovements::AssetItemId)
                        .to(FixedAssetItems::Table, FixedAssetItems::Id)
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
                .name("idx_fixed_asset_movements_item")
                .table(FixedAssetMovements::Table)
                .col(FixedAssetMovements::AssetItemId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in FIXED_ASSET_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("fixed_asset");
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

/// Stored value: a card number with a transaction ledger behind it. The balance is
/// `SUM(amount)` — never a mutated column — and every row carries `balance_after`
/// like the stock ledger, so a discrepancy points at one row.
///
/// The card number is operator-supplied (printed on the physical card), which is
/// why it is unique but not derived. Selling or reloading records the tender
/// method on the row, so the register close can count cash taken for stored
/// value later. Nothing leaves the shelf, so no stock moves: value is stored,
/// not goods handed over.
///
/// Refund-to-card is still open: it needs a card target on the return flow.
async fn gift_cards(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

    manager
        .create_table(
            Table::create()
                .table(GiftCards::Table)
                .if_not_exists()
                .col(ColumnDef::new(GiftCards::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(GiftCards::CardNo).string().not_null().unique_key().to_owned())
                .col(ColumnDef::new(GiftCards::Pin).string().null())
                .col(ColumnDef::new(GiftCards::CreatedBy).integer().null())
                .col(ColumnDef::new(GiftCards::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(GiftCards::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(GiftCardTransactions::Table)
                .if_not_exists()
                .col(ColumnDef::new(GiftCardTransactions::Id).integer().not_null().auto_increment().primary_key().to_owned())
                .col(ColumnDef::new(GiftCardTransactions::GiftCardId).integer().not_null())
                .col(ColumnDef::new(GiftCardTransactions::SaleId).integer().null())
                // `Sell`, `Reload` or `Redeem`. Signed `amount`: in is positive.
                .col(ColumnDef::new(GiftCardTransactions::Kind).string().not_null())
                .col(ColumnDef::new(GiftCardTransactions::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(GiftCardTransactions::BalanceAfter).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(GiftCardTransactions::PaymentMethod).string().null())
                .col(ColumnDef::new(GiftCardTransactions::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_gift_card_transactions_card")
                        .from(GiftCardTransactions::Table, GiftCardTransactions::GiftCardId)
                        .to(GiftCards::Table, GiftCards::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .foreign_key(
                    &mut ForeignKey::create()
                        .name("fk_gift_card_transactions_sale")
                        .from(GiftCardTransactions::Table, GiftCardTransactions::SaleId)
                        .to(Sales::Table, Sales::Id)
                        .on_delete(ForeignKeyAction::SetNull)
                        .to_owned(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .if_not_exists()
                .name("idx_gift_card_transactions_card_id")
                .table(GiftCardTransactions::Table)
                .col(GiftCardTransactions::GiftCardId)
                .to_owned(),
        )
        .await?;

    let conn = manager.get_connection();
    let now = now();
    for name in GIFT_CARD_PERMISSIONS {
        let exists = permissions::Entity::find()
            .filter(permissions::Column::Name.eq(*name))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }
        let group = name.split_once('-').map(|(g, _)| g).unwrap_or("gift_card");
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

/// Repair tickets and paid repair jobs. A warranty is a product sent back through
/// the pipeline (customer → vendor → customer); a servicing is a repair the shop
/// bills for. Both name the product as free text like the reference — the unit on
/// the bench is not necessarily a catalog row anymore.
async fn warranty_and_servicing(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Warranties::ReceivingDate).custom(ts).not_null())
                .col(ColumnDef::new(Warranties::DeliveryDate).custom(ts).null())
                .col(ColumnDef::new(Warranties::CurrentStatus).string().not_null().default("R_F_C"))
                .col(ColumnDef::new(Warranties::TechnicianId).integer().null())
                .col(ColumnDef::new(Warranties::PresentLocation).string().null())
                .col(ColumnDef::new(Warranties::SenderServiceCenter).string().null())
                .col(ColumnDef::new(Warranties::ReceiverServiceCenter).string().null())
                .col(ColumnDef::new(Warranties::CreatedBy).integer().null())
                .col(ColumnDef::new(Warranties::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Warranties::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Warranties::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(Servicings::ReceivingDate).custom(ts).not_null())
                .col(ColumnDef::new(Servicings::DeliveryDate).custom(ts).null())
                .col(ColumnDef::new(Servicings::ServicingCharge).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(Servicings::PaidAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Servicings::CurrentStatus).string().not_null().default("Received"))
                .col(ColumnDef::new(Servicings::TechnicianId).integer().null())
                .col(ColumnDef::new(Servicings::CreatedBy).integer().null())
                .col(ColumnDef::new(Servicings::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Servicings::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Servicings::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(InstallmentSales::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(InstallmentSales::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(InstallmentSaleDetails::DueDate).custom(ts).not_null())
                .col(ColumnDef::new(InstallmentSaleDetails::Amount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null())
                .col(ColumnDef::new(InstallmentSaleDetails::PaidAmount).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(InstallmentSaleDetails::PaidDate).custom(ts).null())
                .col(ColumnDef::new(InstallmentSaleDetails::PaymentMethod).string().null())
                .col(ColumnDef::new(InstallmentSaleDetails::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(InstallmentSaleDetails::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Promotions::StartAt).custom(ts).not_null())
                .col(ColumnDef::new(Promotions::EndAt).custom(ts).not_null())
                .col(ColumnDef::new(Promotions::DelStatus).string().not_null().default(DEL_LIVE))
                .col(ColumnDef::new(Promotions::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Promotions::UpdatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(Quotations::QuotedAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Quotations::ReferenceNo).string().null())
                .col(ColumnDef::new(Quotations::Subtotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Quotations::DiscountTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Quotations::GrandTotal).decimal_len(DECIMAL_PRECISION, DECIMAL_SCALE).not_null().default(0))
                .col(ColumnDef::new(Quotations::CreatedBy).integer().null())
                .col(ColumnDef::new(Quotations::Note).string().null())
                .col(ColumnDef::new(Quotations::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(SalePayments::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
    let ts = timestamp_type(manager.get_database_backend());

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
                .col(ColumnDef::new(CustomerReceives::PaidAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(CustomerReceives::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
                .col(ColumnDef::new(SupplierPayments::PaidAt).custom(ts).not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(SupplierPayments::CreatedAt).custom(ts).not_null().default(Expr::current_timestamp()))
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
enum ServiceRatings {
    Table,
    Id,
    SaleId,
    Rating,
    CreatedAt,
}

/// Permissions this migration owns, for the down arm above.
const RATING_PERMISSIONS: &[&str] = &["rating-submit", "rating-list"];

#[derive(Iden)]
enum CreditNotes {
    Table,
    Id,
    CreditNo,
    CustomerId,
    SaleReturnId,
    Amount,
    AppliedTotal,
    CreatedBy,
    Note,
    DelStatus,
    CreatedAt,
}

/// Permissions this migration owns, for the down arm above.
const PURCHASE_PERMISSIONS: &[&str] = &[
    "purchase-list",
    "purchase-create",
    "purchase-show",
    "purchase-payment",
    "purchase-return-list",
    "purchase-return-create",
    "purchase-return-show",
];

/// Permissions this migration owns, for the down arm above.
const RECURRING_PERMISSIONS: &[&str] = &[
    "recurring-expense-list",
    "recurring-expense-create",
    "recurring-expense-edit",
    "recurring-expense-delete",
    "recurring-expense-post",
];

const ACCOUNTING_PERMISSIONS: &[&str] = &[
    "income-list",
    "income-create",
    "income-show",
    "expense-list",
    "expense-create",
    "expense-show",
    "deposit-withdraw-list",
    "deposit-withdraw-create",
    "accounting-balance",
    "accounting-report",
];

const CREDIT_NOTE_PERMISSIONS: &[&str] = &[
    "creditnote-list",
    "creditnote-issue",
    "creditnote-show",
];

#[derive(Iden)]
enum IncomeCategories {
    Table,
    Id,
    Name,
    Description,
    DelStatus,
    CreatedAt,
}

#[derive(Iden)]
enum Incomes {
    Table,
    Id,
    ReferenceNo,
    IncomeCategoryId,
    PaymentMethodId,
    EmployeeId,
    Amount,
    OccurredAt,
    Note,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum ExpenseCategories {
    Table,
    Id,
    Name,
    Description,
    DelStatus,
    CreatedAt,
}

#[derive(Iden)]
enum Expenses {
    Table,
    Id,
    ReferenceNo,
    ExpenseCategoryId,
    RecurringExpenseId,
    PaymentMethodId,
    EmployeeId,
    Amount,
    OccurredAt,
    Note,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum ExpenseRecurrings {
    Table,
    Id,
    ExpenseCategoryId,
    Name,
    Amount,
    PaymentMethodId,
    Rotation,
    StartsOn,
    NextDueOn,
    EndsOn,
    Note,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum DepositWithdraws {
    Table,
    Id,
    ReferenceNo,
    PaymentMethodId,
    Kind,
    Amount,
    OccurredAt,
    Note,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum LoyaltyEntries {
    Table,
    Id,
    CustomerId,
    SaleId,
    Kind,
    Points,
    CreatedAt,
}

/// Permissions this migration owns, for the down arm above.
const APPROVAL_PERMISSIONS: &[&str] = &["sale-approve"];

#[derive(Iden)]
enum GiftCards {
    Table,
    Id,
    CardNo,
    Pin,
    CreatedBy,
    DelStatus,
    CreatedAt,
}

#[derive(Iden)]
enum GiftCardTransactions {
    Table,
    Id,
    GiftCardId,
    SaleId,
    Kind,
    Amount,
    BalanceAfter,
    PaymentMethod,
    CreatedAt,
}

/// Permissions this migration owns, for the down arm above.
const GIFT_CARD_PERMISSIONS: &[&str] = &[
    "giftcard-list",
    "giftcard-sell",
    "giftcard-reload",
    "giftcard-show",
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
const FIXED_ASSET_PERMISSIONS: &[&str] = &[
    "fixed_asset-list",
    "fixed_asset-create",
    "fixed_asset-destroy",
];

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
    ApprovedBy,
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
    PaymentMethodId,
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
    PaymentMethodId,
    Reference,
    PaidAt,
    CreatedAt,
}

#[derive(Iden)]
enum SupplierPayments {
    Table,
    Id,
    SupplierId,
    PurchaseId,
    PaymentMethodId,
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

/// How money is tendered. A closed master table rather than free text on each
/// payment row, so a report can group payments and Stage 7's income, expense and
/// deposit/withdraw rows point at the same vocabulary.
#[derive(Iden)]
enum PaymentMethods {
    Table,
    Id,
    Name,
    Kind,
    DelStatus,
    CreatedAt,
}

/// A purchase order received from a supplier. Goods arrive here; what was paid
/// is `supplier_payments`, so there is no `paid` or `due` column that can
/// disagree with them.
#[derive(Iden)]
enum Purchases {
    Table,
    Id,
    ReferenceNo,
    SupplierId,
    SupplierInvoiceNo,
    PurchasedAt,
    Subtotal,
    Discount,
    GrandTotal,
    Note,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum PurchaseDetails {
    Table,
    Id,
    PurchaseId,
    ItemId,
    BatchId,
    Quantity,
    UnitPrice,
    Total,
}

#[derive(Iden)]
enum PurchaseReturns {
    Table,
    Id,
    ReferenceNo,
    PurchaseId,
    SupplierId,
    ReturnedAt,
    TotalAmount,
    Note,
    CreatedBy,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum PurchaseReturnDetails {
    Table,
    Id,
    PurchaseReturnId,
    PurchaseDetailId,
    ItemId,
    Quantity,
    UnitPrice,
    Total,
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
    PinHash,
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
enum ItemSubCategories {
    Table,
    Id,
    CategoryId,
    Name,
    Description,
    SortId,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Employees {
    Table,
    Id,
    Name,
    Phone,
    Email,
    Address,
    UserId,
    BaseSalary,
    HireDate,
    TerminatedOn,
    Note,
    CreatedBy,
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
    SubCategoryId,
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
    ParentId,
    Symbology,
    Weighed,
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
    PaymentMethodId,
    CustomerId,
    OrderType,
    Rounding,
    ApprovedBy,
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
    BatchId,
    MovementType,
    Quantity,
    Reference,
    BalanceAfter,
    CreatedAt,
}

#[derive(Iden)]
enum ItemBatches {
    Table,
    Id,
    ItemId,
    BatchNo,
    ExpiryDate,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum FixedAssetItems {
    Table,
    Id,
    Name,
    Code,
    Description,
    PurchasePrice,
    SalePrice,
    DelStatus,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum FixedAssetMovements {
    Table,
    Id,
    AssetItemId,
    MovementKind,
    Quantity,
    UnitPrice,
    Amount,
    ReferenceNo,
    Note,
    CreatedAt,
}



