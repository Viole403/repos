// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod auth;
mod commands;
mod commands_auth;
mod db;
mod entities;
mod migration;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Async work can't run on the sync setup hook, so block the thread
            // until migrations finish. This happens once, before the window opens.
            let handle = app.handle().clone();
            let data_dir = handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|e| panic!("failed to resolve app data dir: {e}"));

            let url_override = std::env::var("REPOS_DATABASE_URL").ok();
            tauri::async_runtime::block_on(db::init(&data_dir, url_override.as_deref()))
                .expect("failed to initialize database");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::list_units,
            commands::create_unit,
            commands::delete_unit,
            commands::list_brands,
            commands::create_brand,
            commands::delete_brand,
            commands::list_item_categories,
            commands::create_item_category,
            commands::delete_item_category,
            commands::list_items,
            commands::create_item,
            commands::update_item,
            commands::delete_item,
            commands::list_stock_movements,
            commands::stock_on_hand,
            commands::checkout,
            commands::list_draft_sales,
            commands::promote_draft,
            commands::discard_draft,
            commands::open_register,
            commands::current_register,
            commands::list_registers,
            commands::register_summary,
            commands::close_register,
            commands::create_promotion,
            commands::list_promotions,
            commands::get_promotion,
            commands::update_promotion,
            commands::delete_promotion,
            commands::create_booking,
            commands::list_bookings,
            commands::get_booking,
            commands::update_booking,
            commands::delete_booking,
            commands::create_quotation,
            commands::list_quotations,
            commands::get_quotation,
            commands::update_quotation,
            commands::delete_quotation,
            commands_auth::login,
            commands_auth::logout,
            commands_auth::current_user,
            commands_auth::install_status,
            commands_auth::list_users,
            commands_auth::create_user,
            commands_auth::delete_user,
            commands_auth::list_roles,
            commands_auth::set_user_role,
            commands_auth::create_role,
            commands_auth::set_role_permissions,
            commands_auth::delete_role,
            commands::list_sales,
            commands::list_sale_payments,
            commands::get_sale,
            commands::create_return,
            commands::list_returns,
            commands::list_customers,
            commands::create_customer,
            commands::update_customer,
            commands::delete_customer,
            commands::customer_balance,
            commands::record_customer_receipt,
            commands::list_customer_receipts,
            commands::list_suppliers,
            commands::create_supplier,
            commands::update_supplier,
            commands::delete_supplier,
            commands::supplier_balance,
            commands::record_supplier_payment,
            commands::list_supplier_payments,
            commands_auth::has_permission,
            commands_auth::my_permissions,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    //! Tests run against a real, migrated database rather than a mock, because the
    //! two things most likely to break silently here are the schema itself and the
    //! atomicity of checkout — neither of which a mock would exercise.
    use super::*;
    use crate::commands::{CheckoutInput, CheckoutLine, PageQuery};
    use crate::entities::auth::permissions;
    use crate::entities::auth::users;
    use crate::entities::catalog::{item, unit};
    use crate::migration::Migrator;
    use sea_orm_migration::MigratorTrait;
    use crate::entities::sales::stock_movement::MovementType;
    use crate::entities::sales::{booking, quotation, quotation_detail, sale, sale_detail, stock_movement};
    use crate::entities::trade::{customer, supplier, supplier_payment};

    fn days_ago(n: i64) -> chrono::NaiveDateTime {
        crate::migration::now() - chrono::Duration::days(n)
    }

    /// A cashier row for FK-scoped tests. The hash is a placeholder: nothing here
    /// signs in, and paying Argon2's cost per test would slow the suite for no
    /// coverage.
    async fn seed_user(db: &DatabaseConnection) -> i32 {
        let now = crate::migration::now();
        users::ActiveModel {
            name: Set("Cashier".into()),
            email: Set(format!("cashier-{}@till.test", now.and_utc().timestamp_micros())),
            password_hash: Set("not-a-real-hash".into()),
            del_status: Set("Live".into()),
            two_factor_enabled: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed user")
        .id
    }
    use sea_orm::prelude::Decimal;
    use sea_orm::ActiveValue::Set;
    use sea_orm::{
        ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait,
        QueryFilter, QueryOrder,
    };

    #[tokio::test]
    async fn migrations_apply_and_query() {
        let db = db::init_for_tests().await;

        // Tables exist and are empty.
        assert_eq!(unit::Entity::find().count(&db).await.unwrap(), 0);

        // Insert, then confirm the soft-delete filter hides it.
        let now = migration::now();
        unit::ActiveModel {
            unit_name: Set("Piece".into()),
            description: Set(None),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        assert_eq!(unit::Entity::find().count(&db).await.unwrap(), 1);

        let row = unit::Entity::find()
            .filter(unit::Column::UnitName.eq("Piece"))
            .one(&db)
            .await
            .unwrap()
            .expect("unit inserted");
        let mut model: unit::ActiveModel = row.into();
        model.del_status = Set("Deleted".into());
        model.update(&db).await.unwrap();

        assert_eq!(
            unit::Entity::find()
                .filter(unit::Column::DelStatus.eq("Live"))
                .count(&db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(unit::Entity::find().count(&db).await.unwrap(), 1);
    }


    #[tokio::test]
    async fn item_table_exists() {
        let db = db::init_for_tests().await;
        assert_eq!(item::Entity::find().count(&db).await.unwrap(), 0);
    }

    /// The permission catalog is seeded by migration, and `permission_names_in`
    /// resolves against these rows — so an empty table means every user holds no
    /// permissions and a guard locks out every operator, Master included.
    #[tokio::test]
    async fn the_permission_catalog_is_seeded() {
        let db = db::init_for_tests().await;

        let all = permissions::Entity::find().all(&db).await.unwrap();
        assert!(!all.is_empty(), "the catalog must not be empty");

        // Every row is `group-action` and `group_name` matches the name's prefix,
        // which is what the settings UI groups on.
        for row in &all {
            let (group, action) = row
                .name
                .split_once('-')
                .unwrap_or_else(|| panic!("permission {} is not group-action", row.name));
            assert!(!action.is_empty(), "permission {} has no action", row.name);
            assert_eq!(
                &row.group_name, group,
                "permission {} disagrees with its group_name",
                row.name
            );
            assert_eq!(row.del_status, "Live");
            assert_eq!(row.guard_name, "web");
        }

        // The permissions the command guards compare against, by name.
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        for wanted in ["item-list", "item-create", "sale-create", "sale-pos", "stock-stock"] {
            assert!(names.contains(&wanted), "missing seeded permission {wanted}");
        }

        // Seeding is idempotent: a second run must not duplicate rows.
        let before = all.len() as u64;
        Migrator::up(&db, None).await.unwrap();
        assert_eq!(
            permissions::Entity::find().count(&db).await.unwrap(),
            before,
            "re-running the migration duplicated permissions"
        );
    }

    // -----------------------------------------------------------------------
    // Sales + the stock ledger
    //
    // `checkout` is reached through its `_in` form: the `#[tauri::command]` shell
    // pulls the process-wide connection that only exists inside the Tauri window,
    // while the logic takes one as an argument and so works against a throwaway
    // in-memory database.
    // -----------------------------------------------------------------------

    /// An item with a code, since `items.code` is the only unique column.
    async fn seed_customer(db: &DatabaseConnection, name: &str, credit_limit: Decimal) -> i32 {
        let now = migration::now();
        customer::ActiveModel {
            name: Set(name.to_owned()),
            credit_limit: Set(credit_limit),
            loyalty_points: Set(Decimal::ZERO),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed customer")
        .id
    }

    /// A completed sale for one customer. `checkout` would be the real writer, but the
    /// balance only cares about status and total.
    async fn seed_customer_sale(db: &DatabaseConnection, customer_id: i32, total: Decimal, status: &str) -> i32 {
        let now = migration::now();
        sale::ActiveModel {
            invoice_no: Set(format!("PENDING-{customer_id}-{total}")),
            status: Set(status.to_owned()),
            subtotal: Set(total),
            discount_total: Set(Decimal::ZERO),
            tax_total: Set(Decimal::ZERO),
            grand_total: Set(total),
            paid_total: Set(total),
            payment_method: Set("Cash".into()),
            customer_id: Set(Some(customer_id)),
            note: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed sale")
        .id
    }

    async fn seed_sale(
        db: &DatabaseConnection,
        at: chrono::NaiveDateTime,
        total: Decimal,
        customer_id: Option<i32>,
        status: &str,
    ) -> i32 {
        let invoice = format!("PENDING-{at}-{total:?}");
        sale::ActiveModel {
            invoice_no: Set(invoice),
            status: Set(status.to_owned()),
            subtotal: Set(total),
            discount_total: Set(Decimal::ZERO),
            tax_total: Set(Decimal::ZERO),
            grand_total: Set(total),
            paid_total: Set(total),
            payment_method: Set("Cash".into()),
            customer_id: Set(customer_id),
            note: Set(None),
            created_at: Set(at),
            updated_at: Set(at),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed sale")
        .id
    }

    fn page_one() -> commands::PageQuery {
        commands::PageQuery { page: 1, per_page: 50, search: None }
    }

    fn no_filter() -> commands::SaleFilter {
        commands::SaleFilter::default()
    }

    async fn seed_item(db: &DatabaseConnection, name: &str) -> i32 {
        let now = migration::now();
        item::ActiveModel {
            name: Set(name.to_owned()),
            code: Set(format!("code-{name}")),
            conversion_rate: Set(Decimal::ONE),
            purchase_price: Set(Decimal::ZERO),
            sale_price: Set(Decimal::ZERO),
            loyalty_point: Set(Decimal::ZERO),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed item")
        .id
    }

    /// Stock arrives through the ledger, exactly as a goods receipt would. This is
    /// also the only way to give an item stock in these tests, which is the point:
    /// on-hand is derived, so writing it any other way would be impossible.
    async fn seed_stock(db: &DatabaseConnection, item_id: i32, quantity: Decimal) {
        stock_movement::ActiveModel {
            item_id: Set(item_id),
            sale_id: Set(None),
            movement_type: Set(MovementType::OpeningBalance.as_str().to_owned()),
            quantity: Set(quantity),
            reference: Set(Some("opening".into())),
            balance_after: Set(quantity),
            created_at: Set(migration::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed opening balance");
    }

    /// A checkout that totals `unit_price * quantity` on one item, with the caller
    /// supplying the payment shape. Keeps the split-payment tests to the part they are
    /// actually about.
    async fn sell_one_item(
        db: &DatabaseConnection,
        item_id: i32,
        unit_price: Decimal,
        paid_total: Option<Decimal>,
        payments: Option<Vec<commands::PaymentLine>>,
    ) -> Result<commands::SaleView, commands::CmdError> {
        commands::checkout_in(
            db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id,
                    quantity: Decimal::new(1_000, 3),
                    unit_price,
                    discount: None,
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: None,
                payments,
            },
        )
        .await
    }

    /// A completed sale of one item, returning its sale id and its single line id.
    async fn sell_one(db: &DatabaseConnection, item_id: i32, quantity: Decimal, price: Decimal) -> (i32, i32) {
        let view = commands::checkout_in(
            db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine { item_id, quantity, unit_price: price, discount: None }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout");
        (view.sale.id, view.lines[0].id)
    }

    fn return_line(sale_detail_id: i32, quantity: Decimal) -> commands::ReturnInput {
        commands::ReturnInput {
            sale_id: 0,
            reason: "Damaged".into(),
            note: None,
            lines: vec![commands::ReturnLine { sale_detail_id, quantity }],
        }
    }

    fn line(item_id: i32, quantity: Decimal, unit_price: Decimal) -> CheckoutLine {
        CheckoutLine {
            item_id,
            quantity,
            unit_price,
            discount: None,
        }
    }

    fn dec(v: i64) -> Decimal {
        Decimal::from(v)
    }

    /// Save a cart as a `Draft` and return its id — a checkout interrupted before
    /// payment. Shorthand for the promotion tests, which all need one.
    async fn draft(db: &DatabaseConnection, lines: Vec<CheckoutLine>) -> i32 {
        commands::checkout_in(
            db,
            CheckoutInput {
                lines,
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: Some(false),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("draft checkout succeeds")
        .sale
        .id
    }

    /// Ledger rows attributable to one sale. Filtered in Rust rather than SQL because
    /// `sale_id` is nullable and this reads clearer than an `IS NOT NULL` predicate.
    async fn movements_for_sale(
        db: &DatabaseConnection,
        sale_id: i32,
    ) -> Vec<stock_movement::Model> {
        stock_movement::Entity::find()
            .order_by_asc(stock_movement::Column::Id)
            .all(db)
            .await
            .expect("read the ledger")
            .into_iter()
            .filter(|m| m.sale_id == Some(sale_id))
            .collect()
    }

    #[tokio::test]
    async fn checkout_writes_the_sale_its_lines_and_the_movements() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        let biscuit = seed_item(&db, "Biscuit").await;
        seed_stock(&db, cola, dec(10)).await;
        seed_stock(&db, biscuit, dec(4)).await;

        let view = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![
                    line(cola, dec(2), dec(5000)),
                    line(biscuit, dec(1), dec(2000)),
                ],
                discount_total: None,
                tax_total: Some(dec(500)),
                paid_total: Some(dec(13000)),
                payment_method: Some("Qris".into()),
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout succeeds");

        // The header carries the derived totals, and the invoice number is the one
        // derived from the primary key rather than the provisional placeholder.
        assert_eq!(view.sale.status, "Completed");
        assert_eq!(view.sale.invoice_no, format!("INV-{:06}", view.sale.id));
        assert!(!view.sale.invoice_no.starts_with("PENDING"));
        assert_eq!(view.sale.subtotal, dec(12000));
        assert_eq!(view.sale.tax_total, dec(500));
        assert_eq!(view.sale.grand_total, dec(12500));
        assert_eq!(view.sale.paid_total, dec(13000));
        assert_eq!(view.sale.payment_method, "Qris");

        // Both lines persisted, with the name snapshotted off the item row.
        assert_eq!(view.lines.len(), 2);
        assert_eq!(sale_detail::Entity::find().count(&db).await.unwrap(), 2);
        let cola_line = sale_detail::Entity::find()
            .filter(sale_detail::Column::ItemId.eq(cola))
            .one(&db)
            .await
            .unwrap()
            .expect("cola line");
        assert_eq!(cola_line.item_name, "Cola");
        assert_eq!(cola_line.line_total, dec(10000));
        assert_eq!(cola_line.sale_id, view.sale.id);

        // One negative ledger row per line, pointing at the sale.
        let movements = stock_movement::Entity::find()
            .order_by_asc(stock_movement::Column::Id)
            .all(&db)
            .await
            .unwrap();
        let sale_movements: Vec<_> = movements
            .iter()
            .filter(|m| m.sale_id == Some(view.sale.id))
            .collect();
        assert_eq!(sale_movements.len(), 2);
        assert!(sale_movements.iter().all(|m| m.movement_type == "Sale"));
        assert!(sale_movements.iter().all(|m| m.quantity < Decimal::ZERO));
        assert!(sale_movements.iter().all(|m| m.reference.as_deref()
            == Some(view.sale.invoice_no.as_str())));

        // `balance_after` is the running on-hand: 10 - 2 for cola, 4 - 1 for biscuit.
        // Newest first, so this is the sale's row rather than the opening balance.
        let cola_movement = stock_movement::Entity::find()
            .filter(stock_movement::Column::ItemId.eq(cola))
            .order_by_desc(stock_movement::Column::Id)
            .one(&db)
            .await
            .unwrap()
            .expect("cola movement");
        assert_eq!(cola_movement.balance_after, dec(8));
        assert_eq!(cola_movement.quantity, dec(-2));

        // The view carries the post-sale on-hand so the register needs no extra call.
        assert_eq!(view.stock_on_hand.len(), 2);
    }

    /// The property that matters most: a failure anywhere leaves nothing behind.
    /// A sale row without its lines, or a decrement without its sale, is the bug
    /// the transaction exists to prevent.
    #[tokio::test]
    async fn checkout_is_atomic() {
        let db = db::init_for_tests().await;
        let real = seed_item(&db, "Real").await;
        seed_stock(&db, real, dec(10)).await;

        let result = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![
                    line(real, dec(1), dec(1000)),
                    // No such item: the loop has already written the header and the
                    // first line when this is reached.
                    line(9999, dec(1), dec(1000)),
                ],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await;

        assert!(result.is_err(), "checkout must reject an unknown item");
        assert_eq!(sale::Entity::find().count(&db).await.unwrap(), 0, "no sale row");
        assert_eq!(
            sale_detail::Entity::find().count(&db).await.unwrap(),
            0,
            "no orphaned line"
        );
        // Only the seeded opening balance survives; nothing was written for the sale.
        assert_eq!(
            stock_movement::Entity::find().count(&db).await.unwrap(),
            1,
            "no orphaned movement"
        );
        assert_eq!(
            commands::stock_on_hand_in(&db, real).await.unwrap(),
            dec(10),
            "stock unchanged"
        );
    }

    #[tokio::test]
    async fn stock_on_hand_drops_by_the_sold_quantity() {
        let db = db::init_for_tests().await;
        let soap = seed_item(&db, "Soap").await;
        seed_stock(&db, soap, dec(25)).await;

        assert_eq!(
            commands::stock_on_hand_in(&db, soap).await.unwrap(),
            dec(25)
        );

        commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(soap, dec(4), dec(3000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout succeeds");

        assert_eq!(
            commands::stock_on_hand_in(&db, soap).await.unwrap(),
            dec(21)
        );

        // The derived figure is the sum of signed movements, not a stored number:
        // the opening balance is still there, with the sale's decrement on top.
        assert_eq!(stock_movement::Entity::find().count(&db).await.unwrap(), 2);
    }

    /// An item that has never moved reports 0 rather than NULL — `SUM` over no rows
    /// is NULL, and the command supplies the zero.
    #[tokio::test]
    async fn stock_on_hand_is_zero_without_movements() {
        let db = db::init_for_tests().await;
        let ghost = seed_item(&db, "Ghost").await;
        assert_eq!(
            commands::stock_on_hand_in(&db, ghost).await.unwrap(),
            Decimal::ZERO
        );
    }

    #[tokio::test]
    async fn checkout_rejects_a_non_positive_quantity() {
        let db = db::init_for_tests().await;
        let nail = seed_item(&db, "Nail").await;
        seed_stock(&db, nail, dec(100)).await;

        for bad in [Decimal::ZERO, dec(-1)] {
            let err = commands::checkout_in(
                &db,
                CheckoutInput {
                    lines: vec![line(nail, bad, dec(500))],
                    discount_total: None,
                    tax_total: None,
                    paid_total: None,
                    payment_method: None,
                    note: None,
                    promote: None,
                    customer_id: None,
                    payments: None,
                },
            )
            .await
            .expect_err("quantity must be positive");
            assert!(
                format!("{err}").contains("quantity must be greater than zero"),
                "unexpected error: {err}"
            );
        }

        // An empty cart is rejected too.
        assert!(commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            }
        )
        .await
        .is_err());

        assert_eq!(sale::Entity::find().count(&db).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn checkout_rejects_insufficient_stock() {
        let db = db::init_for_tests().await;
        let tv = seed_item(&db, "Television").await;
        seed_stock(&db, tv, dec(2)).await;

        let err = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(tv, dec(3), dec(500000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect_err("2 in stock cannot cover a sale of 3");
        assert!(
            format!("{err}").contains("in stock but"),
            "unexpected error: {err}"
        );

        // Rolled back, so the ledger still shows the untouched opening balance.
        assert_eq!(sale::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(
            commands::stock_on_hand_in(&db, tv).await.unwrap(),
            dec(2)
        );
    }

    /// A draft is a saved cart, not a transaction: it must not shrink the shelf.
    #[tokio::test]
    async fn a_draft_checkout_writes_no_stock_movements() {
        let db = db::init_for_tests().await;
        let mug = seed_item(&db, "Mug").await;
        seed_stock(&db, mug, dec(7)).await;

        let view = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(mug, dec(2), dec(15000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: Some(false),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("draft checkout succeeds");

        assert_eq!(view.sale.status, "Draft");
        assert_eq!(view.lines.len(), 1);
        // Only the opening balance is in the ledger.
        assert_eq!(stock_movement::Entity::find().count(&db).await.unwrap(), 1);
        assert_eq!(
            commands::stock_on_hand_in(&db, mug).await.unwrap(),
            dec(7)
        );
        // Nothing to report back, because nothing moved.
        assert!(view.stock_on_hand.is_empty());
    }

    /// A draft is still checked against stock. That is a judgement call rather than
    /// an obvious consequence of "a draft moves nothing": it keeps a saved cart a
    /// truthful statement about what could actually be sold, and promotion re-checks
    /// anyway. Pinned here so it cannot be changed by accident — if holds turn out
    /// to need to survive a stock-out, this is the test to change.
    #[tokio::test]
    async fn a_draft_checkout_is_still_stock_checked() {
        let db = db::init_for_tests().await;
        let last = seed_item(&db, "LastOne").await;
        seed_stock(&db, last, dec(1)).await;

        let err = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(last, dec(5), dec(999))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: Some(false),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect_err("a draft cannot hold more than is on the shelf");

        assert!(format!("{err}").contains("in stock but"), "got: {err}");
        assert_eq!(sale::Entity::find().count(&db).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn list_stock_movements_is_newest_first_and_scoped_to_the_item() {
        let db = db::init_for_tests().await;
        let rice = seed_item(&db, "Rice").await;
        let oil = seed_item(&db, "Oil").await;
        seed_stock(&db, rice, dec(10)).await;
        seed_stock(&db, oil, dec(3)).await;

        commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(rice, dec(1), dec(12000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout succeeds");

        let page = commands::list_stock_movements_in(
            &db,
            rice,
            PageQuery {
                page: 1,
                per_page: 50,
                search: None,
            },
        )
        .await
        .expect("list movements");

        // The oil ledger rows must not leak into the rice history.
        assert_eq!(page.total, 2);
        assert_eq!(page.rows.len(), 2);
        assert!(page.rows.iter().all(|m| m.item_id == rice));
        assert!(page.rows[0].id > page.rows[1].id, "newest movement first");
        assert_eq!(page.rows[0].quantity, dec(-1));
    }

    // -----------------------------------------------------------------------
    // The draft lifecycle
    //
    // `checkout(promote: false)` writes a recoverable draft and deliberately no stock
    // movements. These cover the half that makes that promise real: reading the draft
    // back, promoting it, and discarding it.
    // -----------------------------------------------------------------------

    /// A crashed checkout leaves a draft. This is the read-back that makes the crash
    /// recoverable — without it the draft is unreachable and the promise `checkout`
    /// makes by writing one is empty.
    #[tokio::test]
    async fn a_draft_is_listed_for_resume_with_its_lines() {
        let db = db::init_for_tests().await;
        let tea = seed_item(&db, "Tea").await;
        let sugar = seed_item(&db, "Sugar").await;
        seed_stock(&db, tea, dec(10)).await;
        seed_stock(&db, sugar, dec(10)).await;

        let first = draft(
            &db,
            vec![line(tea, dec(2), dec(5000)), line(sugar, dec(1), dec(3000))],
        )
        .await;
        let second = draft(&db, vec![line(tea, dec(1), dec(5000))]).await;

        // A completed sale is not resumable, so it must not appear here at all.
        commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(sugar, dec(1), dec(3000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout succeeds");

        let drafts = commands::list_draft_sales_in(&db).await.expect("list drafts");

        assert_eq!(drafts.len(), 2, "only drafts are resumable");
        // Newest first, so the crashed cart is the top row.
        assert_eq!(drafts[0].sale.id, second);
        assert_eq!(drafts[1].sale.id, first);
        assert!(drafts.iter().all(|d| d.sale.status == "Draft"));

        // Both of the older draft's lines come back, in cart order.
        let older = drafts.iter().find(|d| d.sale.id == first).expect("the first draft");
        assert_eq!(older.lines.len(), 2);
        assert_eq!(older.lines[0].item_id, tea);
        assert_eq!(older.lines[0].quantity, dec(2));
        assert_eq!(older.lines[1].item_id, sugar);
    }

    /// Promotion is the half of checkout a draft skipped: the ledger rows land, the
    /// header becomes `Completed`, and the shelf agrees.
    #[tokio::test]
    async fn promote_draft_completes_the_sale_and_moves_stock() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        let biscuit = seed_item(&db, "Biscuit").await;
        seed_stock(&db, cola, dec(10)).await;
        seed_stock(&db, biscuit, dec(4)).await;

        let sale_id = draft(
            &db,
            vec![line(cola, dec(2), dec(5000)), line(biscuit, dec(1), dec(2000))],
        )
        .await;

        let view = commands::promote_draft_in(&db, sale_id, Some(dec(13000)), Some("Qris".into()))
            .await
            .expect("promotion succeeds");

        assert_eq!(view.sale.id, sale_id);
        assert_eq!(view.sale.status, "Completed");
        // Rebuilt from the stored lines rather than read back from the draft header.
        assert_eq!(view.sale.subtotal, dec(12000));
        assert_eq!(view.sale.discount_total, Decimal::ZERO);
        assert_eq!(view.sale.grand_total, dec(12000));
        assert_eq!(view.sale.paid_total, dec(13000));
        assert_eq!(view.sale.payment_method, "Qris");
        // The draft already carried its final invoice number, so the ledger can point
        // at it without a second numbering pass.
        assert_eq!(view.sale.invoice_no, format!("INV-{sale_id:06}"));
        assert_eq!(view.lines.len(), 2);

        // The movements the draft withheld are on the ledger, pointing at the sale.
        let movements = movements_for_sale(&db, sale_id).await;
        assert_eq!(movements.len(), 2);
        assert!(movements.iter().all(|m| m.movement_type == "Sale"));
        assert!(movements.iter().all(|m| m.reference.as_deref() == Some(view.sale.invoice_no.as_str())));
        // `balance_after` is the running on-hand: 10 - 2 for cola, 4 - 1 for biscuit.
        assert_eq!(movements[0].quantity, dec(-2));
        assert_eq!(movements[0].balance_after, dec(8));
        assert_eq!(movements[1].quantity, dec(-1));
        assert_eq!(movements[1].balance_after, dec(3));

        // And the derived shelf agrees with the ledger.
        assert_eq!(commands::stock_on_hand_in(&db, cola).await.unwrap(), dec(8));
        assert_eq!(commands::stock_on_hand_in(&db, biscuit).await.unwrap(), dec(3));

        // The view carries the resulting on-hand, so the register needs no second call.
        assert_eq!(view.stock_on_hand.len(), 2);

        // A completed sale is no longer resumable.
        assert!(commands::list_draft_sales_in(&db).await.unwrap().is_empty());
    }

    /// With no arguments, the draft's own payment method carries over and the sale is
    /// paid in full — which is what a cashier pressing "complete" means.
    #[tokio::test]
    async fn promote_draft_defaults_the_payment_method_and_the_paid_total() {
        let db = db::init_for_tests().await;
        let soap = seed_item(&db, "Soap").await;
        seed_stock(&db, soap, dec(5)).await;

        let sale_id = draft(&db, vec![line(soap, dec(2), dec(3000))]).await;

        let view = commands::promote_draft_in(&db, sale_id, None, None)
            .await
            .expect("promotion succeeds");

        assert_eq!(view.sale.payment_method, "Cash", "the draft's own method");
        assert_eq!(view.sale.paid_total, view.sale.grand_total, "paid in full");
        assert_eq!(view.sale.paid_total, dec(6000));
    }

    /// The double-spend guard, from the sequential side. Asserted against exact
    /// figures: "the number went down" would pass just as happily on a double
    /// decrement as on a correct one.
    #[tokio::test]
    async fn promoting_twice_does_not_double_decrement_stock() {
        let db = db::init_for_tests().await;
        let tv = seed_item(&db, "Television").await;
        seed_stock(&db, tv, dec(5)).await;

        let sale_id = draft(&db, vec![line(tv, dec(2), dec(500000))]).await;

        commands::promote_draft_in(&db, sale_id, None, None)
            .await
            .expect("the first promotion succeeds");
        assert_eq!(commands::stock_on_hand_in(&db, tv).await.unwrap(), dec(3));
        assert_eq!(movements_for_sale(&db, sale_id).await.len(), 1);

        // The second attempt is refused, not silently accepted.
        let err = commands::promote_draft_in(&db, sale_id, None, None)
            .await
            .expect_err("a completed sale cannot be promoted again");
        assert!(
            matches!(err, commands::CmdError::Conflict(_)),
            "unexpected error: {err}"
        );

        // 5 - 2, once. Not 5 - 2 - 2.
        assert_eq!(commands::stock_on_hand_in(&db, tv).await.unwrap(), dec(3));
        assert_eq!(
            movements_for_sale(&db, sale_id).await.len(),
            1,
            "one movement for the sale, not two"
        );
        // The ledger as a whole is still the opening balance plus that single row.
        assert_eq!(stock_movement::Entity::find().count(&db).await.unwrap(), 2);
    }

    /// The same guard, on a sale that was completed outright and was therefore never
    /// a draft: there is nothing to promote and nothing may be written.
    #[tokio::test]
    async fn promote_draft_rejects_a_sale_that_was_never_a_draft() {
        let db = db::init_for_tests().await;
        let mug = seed_item(&db, "Mug").await;
        seed_stock(&db, mug, dec(7)).await;

        let done = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(mug, dec(2), dec(15000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout succeeds")
        .sale
        .id;

        let err = commands::promote_draft_in(&db, done, None, None)
            .await
            .expect_err("a completed sale cannot be promoted");
        assert!(
            matches!(err, commands::CmdError::Conflict(_)),
            "unexpected error: {err}"
        );

        // The original movement is untouched: still exactly one, still for 2 units.
        assert_eq!(movements_for_sale(&db, done).await.len(), 1);
        assert_eq!(commands::stock_on_hand_in(&db, mug).await.unwrap(), dec(5));
    }

    /// A draft waits; stock does not. Promotion re-reads the shelf instead of trusting
    /// what the cart believed when it was scanned.
    #[tokio::test]
    async fn promote_draft_rejects_when_the_stock_is_gone() {
        let db = db::init_for_tests().await;
        let last = seed_item(&db, "LastOne").await;
        seed_stock(&db, last, dec(3)).await;

        let sale_id = draft(&db, vec![line(last, dec(3), dec(9999))]).await;

        // Another till sells the same stock while the draft waits.
        commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(last, dec(2), dec(9999))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("the other till sells");

        let err = commands::promote_draft_in(&db, sale_id, None, None)
            .await
            .expect_err("1 left cannot cover a held line of 3");
        assert!(format!("{err}").contains("in stock but"), "got: {err}");

        // The draft is untouched and still resumable — a refusal must not destroy the
        // cart the cashier was working on.
        let drafts = commands::list_draft_sales_in(&db).await.expect("list drafts");
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].sale.id, sale_id);
        assert_eq!(drafts[0].sale.status, "Draft");
        assert_eq!(
            movements_for_sale(&db, sale_id).await.len(),
            0,
            "a refused promotion writes no movement"
        );
        assert_eq!(commands::stock_on_hand_in(&db, last).await.unwrap(), dec(1));
    }

    /// A draft may have sat for days behind a catalog that changed, so promotion runs
    /// the stored lines back through the *same* validator a fresh checkout passes.
    /// These rows are poked directly because checkout itself cannot produce them.
    #[tokio::test]
    async fn promote_draft_revalidates_the_stored_lines() {
        let db = db::init_for_tests().await;
        let nail = seed_item(&db, "Nail").await;
        seed_stock(&db, nail, dec(100)).await;

        let sale_id = draft(&db, vec![line(nail, dec(5), dec(500))]).await;
        let detail = sale_detail::Entity::find()
            .filter(sale_detail::Column::SaleId.eq(sale_id))
            .one(&db)
            .await
            .unwrap()
            .expect("the draft's line");

        let mut am: sale_detail::ActiveModel = detail.clone().into();
        am.quantity = Set(Decimal::ZERO);
        am.update(&db).await.unwrap();

        let err = commands::promote_draft_in(&db, sale_id, None, None)
            .await
            .expect_err("a zero quantity cannot be promoted");
        assert!(
            format!("{err}").contains("quantity must be greater than zero"),
            "unexpected error: {err}"
        );

        // A negative price, the other half of the same rule.
        let mut am: sale_detail::ActiveModel = detail.into();
        am.quantity = Set(dec(5));
        am.unit_price = Set(dec(-1));
        am.update(&db).await.unwrap();

        let err = commands::promote_draft_in(&db, sale_id, None, None)
            .await
            .expect_err("a negative price cannot be promoted");
        assert!(
            format!("{err}").contains("unit price cannot be negative"),
            "unexpected error: {err}"
        );

        assert_eq!(movements_for_sale(&db, sale_id).await.len(), 0);
        assert_eq!(commands::stock_on_hand_in(&db, nail).await.unwrap(), dec(100));
    }

    /// The two shapes promotion cannot recover from: nothing at that id, and a draft
    /// with no lines. The latter is unreachable through checkout, which refuses an
    /// empty cart, so the row is written directly.
    #[tokio::test]
    async fn promote_draft_rejects_a_missing_or_empty_sale() {
        let db = db::init_for_tests().await;

        let err = commands::promote_draft_in(&db, 9999, None, None)
            .await
            .expect_err("no such sale");
        assert!(
            matches!(err, commands::CmdError::NotFound(_)),
            "unexpected error: {err}"
        );

        let now = migration::now();
        let empty = sale::ActiveModel {
            invoice_no: Set("PENDING-empty".into()),
            status: Set("Draft".into()),
            subtotal: Set(Decimal::ZERO),
            discount_total: Set(Decimal::ZERO),
            tax_total: Set(Decimal::ZERO),
            grand_total: Set(Decimal::ZERO),
            paid_total: Set(Decimal::ZERO),
            payment_method: Set("Cash".into()),
            customer_id: Set(None),
            note: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("insert an empty draft")
        .id;

        // Omitted from the resume list, so it can never be reached from the UI either.
        assert!(commands::list_draft_sales_in(&db).await.unwrap().is_empty());

        let err = commands::promote_draft_in(&db, empty, None, None)
            .await
            .expect_err("a draft with no lines cannot be promoted");
        assert!(
            matches!(err, commands::CmdError::Validation(_)),
            "unexpected error: {err}"
        );
        assert_eq!(movements_for_sale(&db, empty).await.len(), 0);
    }

    /// A draft was never completed, so discarding it is a plain delete: the lines go
    /// with it, and the shelf is never touched because no movement ever existed.
    #[tokio::test]
    async fn discard_draft_removes_the_sale_and_its_lines() {
        let db = db::init_for_tests().await;
        let tea = seed_item(&db, "Tea").await;
        seed_stock(&db, tea, dec(10)).await;

        let sale_id = draft(&db, vec![line(tea, dec(2), dec(5000))]).await;
        assert_eq!(sale_detail::Entity::find().count(&db).await.unwrap(), 1);

        commands::discard_draft_in(&db, sale_id)
            .await
            .expect("discard succeeds");

        assert!(
            sale::Entity::find_by_id(sale_id)
                .one(&db)
                .await
                .unwrap()
                .is_none(),
            "the header is gone"
        );
        assert_eq!(
            sale_detail::Entity::find().count(&db).await.unwrap(),
            0,
            "the lines went with it"
        );
        assert!(commands::list_draft_sales_in(&db).await.unwrap().is_empty());
        // Discarding is not a return: the opening balance is still 10.
        assert_eq!(commands::stock_on_hand_in(&db, tea).await.unwrap(), dec(10));
    }

    /// A completed sale is financial history, so it cannot be discarded — only voided
    /// or refunded, which is a stage away and deliberately not a hard delete.
    #[tokio::test]
    async fn discard_draft_refuses_a_completed_sale() {
        let db = db::init_for_tests().await;
        let mug = seed_item(&db, "Mug").await;
        seed_stock(&db, mug, dec(7)).await;

        let done = commands::checkout_in(
            &db,
            CheckoutInput {
                lines: vec![line(mug, dec(2), dec(15000))],
                discount_total: None,
                tax_total: None,
                paid_total: None,
                payment_method: None,
                note: None,
                promote: None,
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout succeeds")
        .sale
        .id;

        let err = commands::discard_draft_in(&db, done)
            .await
            .expect_err("a completed sale cannot be discarded");
        assert!(
            matches!(err, commands::CmdError::Conflict(_)),
            "unexpected error: {err}"
        );

        // The sale and its movement both survive the refused delete.
        assert!(sale::Entity::find_by_id(done).one(&db).await.unwrap().is_some());
        assert_eq!(sale_detail::Entity::find().count(&db).await.unwrap(), 1);
        assert_eq!(movements_for_sale(&db, done).await.len(), 1);

        let err = commands::discard_draft_in(&db, 9999)
            .await
            .expect_err("no such sale");
        assert!(
            matches!(err, commands::CmdError::NotFound(_)),
            "unexpected error: {err}"
        );
    }

    /// Postgres is the default backend, so the schema has to apply there too — not
    /// just to SQLite. This is the check that backs the portability claim: a column
    /// type SQLite tolerates can still be rejected by Postgres, and `cargo check`
    /// would never see it.
    ///
    /// Set `REPOS_TEST_POSTGRES_URL` to run it, e.g.
    ///   REPOS_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost/repos_test \
    ///     cargo test
    /// Skipped (not failed) when the variable is unset, so the default suite needs no
    /// database server.
    #[tokio::test]
    async fn migrations_apply_to_postgres() {
        let Ok(url) = std::env::var("REPOS_TEST_POSTGRES_URL") else {
            eprintln!("skipping: REPOS_TEST_POSTGRES_URL not set");
            return;
        };

        let db = db::connect_to(&url)
            .await
            .expect("connect to the test Postgres");

        // Both tables exist and are queryable, which means the DDL was accepted.
        assert_eq!(unit::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(item::Entity::find().count(&db).await.unwrap(), 0);

        // The sales DDL is accepted too, and with it the non-unique indexes, which
        // cannot be inlined into CREATE TABLE.
        assert_eq!(sale::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(sale_detail::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(stock_movement::Entity::find().count(&db).await.unwrap(), 0);

        // The Stage 3 tables, which is what this leg was run for: `timestamp` columns
        // only decode into `NaiveDateTime`, so a `DateTimeUtc` field fails here and
        // nowhere else.
        assert_eq!(customer::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(supplier::Entity::find().count(&db).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_customer_with_no_sales_owes_nothing() {
        let db = db::init_for_tests().await;
        let id = seed_customer(&db, "Walk-in", Decimal::ZERO).await;
        assert_eq!(commands::customer_balance_in(&db, id).await.unwrap(), Decimal::ZERO);
    }

    #[tokio::test]
    async fn a_draft_sale_is_not_a_debt() {
        let db = db::init_for_tests().await;
        let id = seed_customer(&db, "Browser", Decimal::ZERO).await;
        seed_customer_sale(&db, id, Decimal::new(250_000, 3), "Draft").await;
        assert_eq!(
            commands::customer_balance_in(&db, id).await.unwrap(),
            Decimal::ZERO,
            "a draft is a basket nobody has paid for, so it is not a debt"
        );
    }

    #[tokio::test]
    async fn a_receipt_reduces_the_balance() {
        let db = db::init_for_tests().await;
        let id = seed_customer(&db, "Regular", Decimal::ZERO).await;
        seed_customer_sale(&db, id, Decimal::new(100_000, 3), "Completed").await;
        seed_customer_sale(&db, id, Decimal::new(150_500, 3), "Completed").await;
        assert_eq!(commands::customer_balance_in(&db, id).await.unwrap(), Decimal::new(250_500, 3));

        commands::record_customer_receipt_in(
            &db,
            id,
            commands::ReceiveInput { amount: Decimal::new(50_500, 3), reference: None, paid_at: None },
        )
        .await
        .expect("record a receipt");

        assert_eq!(
            commands::customer_balance_in(&db, id).await.unwrap(),
            Decimal::new(200_000, 3),
            "a payment that does not reduce the debt is not a payment"
        );
    }

    #[tokio::test]
    async fn paying_more_than_owed_leaves_a_credit_balance() {
        let db = db::init_for_tests().await;
        let id = seed_customer(&db, "Overpayer", Decimal::ZERO).await;
        seed_customer_sale(&db, id, Decimal::new(100_000, 3), "Completed").await;
        commands::record_customer_receipt_in(
            &db,
            id,
            commands::ReceiveInput { amount: Decimal::new(150_000, 3), reference: None, paid_at: None },
        )
        .await
        .expect("record an overpayment");

        assert_eq!(
            commands::customer_balance_in(&db, id).await.unwrap(),
            Decimal::new(-50_000, 3),
            "overpayment is negative, not clamped to zero"
        );
    }

    #[tokio::test]
    async fn a_customer_with_payments_cannot_be_deleted() {
        let db = db::init_for_tests().await;
        let id = seed_customer(&db, "Has paid", Decimal::ZERO).await;
        commands::record_customer_receipt_in(
            &db,
            id,
            commands::ReceiveInput { amount: Decimal::new(10_000, 3), reference: None, paid_at: None },
        )
        .await
        .expect("record a receipt");

        assert!(
            commands::delete_customer_in(&db, id).await.is_err(),
            "deleting them strands the payment history that made the row matter"
        );
    }

    #[tokio::test]
    async fn a_receipt_of_zero_is_refused() {
        let db = db::init_for_tests().await;
        let id = seed_customer(&db, "Zero", Decimal::ZERO).await;
        assert!(
            commands::record_customer_receipt_in(
                &db,
                id,
                commands::ReceiveInput { amount: Decimal::ZERO, reference: None, paid_at: None },
            )
            .await
            .is_err(),
            "a zero receipt is a no-op row that makes the ledger lie"
        );
    }

    #[tokio::test]
    async fn a_supplier_balance_is_the_opening_balance_less_payments() {
        let db = db::init_for_tests().await;
        let now = migration::now();
        let id = supplier::ActiveModel {
            name: Set("Wholesale".into()),
            opening_balance: Set(Decimal::new(5_000_000, 3)),
            del_status: Set("Live".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        assert_eq!(commands::supplier_balance_in(&db, id).await.unwrap(), Decimal::new(5_000_000, 3));

        supplier_payment::ActiveModel {
            supplier_id: Set(id),
            amount: Set(Decimal::new(2_000_000, 3)),
            paid_at: Set(now),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        assert_eq!(
            commands::supplier_balance_in(&db, id).await.unwrap(),
            Decimal::new(3_000_000, 3),
            "a payment to a supplier does not reduce what the shop owes"
        );
    }

    #[tokio::test]
    async fn a_duplicate_customer_code_is_refused() {
        let db = db::init_for_tests().await;
        let input = || commands::CustomerInput {
            name: "Someone".into(),
            code: Some(" CUST-1 ".into()),
            email: None,
            phone: None,
            address: None,
            city: None,
            country: None,
            zip: None,
            tax_number: None,
            credit_limit: Decimal::ZERO,
            loyalty_points: Decimal::ZERO,
            note: None,
        };
        let first = commands::create_customer_in(&db, input()).await.expect("first");
        assert_eq!(first.code.as_deref(), Some("CUST-1"), "the code should be trimmed");

        assert!(
            matches!(
                commands::create_customer_in(&db, input()).await,
                Err(commands::CmdError::Conflict(_))
            ),
            "two customers sharing a member number makes receipts ambiguous"
        );
    }

    #[tokio::test]
    async fn a_sale_can_name_a_customer_and_move_their_balance() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let quantity = Decimal::new(2_000, 3);

        commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id: item,
                    quantity,
                    unit_price: Decimal::new(25_000, 3),
                    discount: None,
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: Some(customer),
                payments: None,
            },
        )
        .await
        .expect("checkout against a customer");

        assert_eq!(
            commands::customer_balance_in(&db, customer).await.unwrap(),
            Decimal::new(50_000, 3),
            "a sale that names a customer does not move what they owe"
        );
    }

    #[tokio::test]
    async fn a_sale_against_an_unknown_customer_is_refused() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        let err = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id: item,
                    quantity: Decimal::new(1_000, 3),
                    unit_price: Decimal::new(5_000, 3),
                    discount: None,
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: Some(4242),
                payments: None,
            },
        )
        .await
        .expect_err("no such customer");

        assert!(matches!(err, commands::CmdError::NotFound(_)), "unexpected error: {err}");
        assert_eq!(
            sale::Entity::find().count(&db).await.unwrap(),
            0,
            "a refused checkout left a sale behind"
        );
    }

    #[tokio::test]
    async fn a_walk_in_sale_names_no_customer() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        let view = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id: item,
                    quantity: Decimal::new(1_000, 3),
                    unit_price: Decimal::new(5_000, 3),
                    discount: None,
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("a walk-in sale");

        assert!(
            view.sale.customer_id.is_none(),
            "a walk-in sale must not be pinned to a placeholder customer"
        );
    }

    #[tokio::test]
    async fn the_sales_list_names_the_customer() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let now = migration::now();
        seed_sale(&db, now, Decimal::new(1_000, 3), Some(customer), "Completed").await;
        seed_sale(&db, now, Decimal::new(2_000, 3), None, "Completed").await;

        let rows = commands::list_sales_in(&db, &no_filter(), &page_one()).await.unwrap().rows;
        assert_eq!(rows.len(), 2);
        let named = rows.iter().find(|r| r.customer_id == Some(customer)).unwrap();
        assert_eq!(named.customer_name.as_deref(), Some("Regular"));
        let walk_in = rows.iter().find(|r| r.customer_id.is_none()).unwrap();
        assert!(walk_in.customer_name.is_none(), "a walk-in sale must not inherit a name");
    }

    #[tokio::test]
    async fn a_date_filter_includes_the_whole_named_day() {
        let db = db::init_for_tests().await;
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
        let morning = day.and_hms_opt(0, 0, 1).unwrap();
        let last_second = day.and_hms_opt(23, 59, 59).unwrap();
        let next_day = day.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap();

        seed_sale(&db, morning, Decimal::new(1_000, 3), None, "Completed").await;
        seed_sale(&db, last_second, Decimal::new(2_000, 3), None, "Completed").await;
        seed_sale(&db, next_day, Decimal::new(3_000, 3), None, "Completed").await;

        let filter = commands::SaleFilter {
            from: Some("2026-10-03".into()),
            to: Some("2026-10-03".into()),
            ..Default::default()
        };
        let rows = commands::list_sales_in(&db, &filter, &page_one()).await.unwrap().rows;

        assert_eq!(
            rows.len(),
            2,
            "a sale at 23:59:59 is inside the day it happened"
        );
    }

    #[tokio::test]
    async fn a_malformed_date_narrows_nothing() {
        let db = db::init_for_tests().await;
        seed_sale(&db, migration::now(), Decimal::new(1_000, 3), None, "Completed").await;

        let filter = commands::SaleFilter { from: Some("03/10/2026".into()), ..Default::default() };
        let rows = commands::list_sales_in(&db, &filter, &page_one()).await.unwrap().rows;
        assert_eq!(rows.len(), 1, "a typo in the date box empties the list instead of ignoring it");
    }

    #[tokio::test]
    async fn a_draft_is_hidden_from_the_sales_list_unless_asked_for() {
        let db = db::init_for_tests().await;
        let now = migration::now();
        seed_sale(&db, now, Decimal::new(1_000, 3), None, "Completed").await;
        seed_sale(&db, now, Decimal::new(2_000, 3), None, "Draft").await;

        let all = commands::list_sales_in(&db, &no_filter(), &page_one()).await.unwrap();
        assert_eq!(all.rows.len(), 2);
        assert_eq!(all.total, 2);

        let completed = commands::list_sales_in(
            &db,
            &commands::SaleFilter { status: Some("Completed".into()), ..Default::default() },
            &page_one(),
        )
        .await
        .unwrap();
        assert_eq!(completed.total, 1);
        assert_eq!(completed.rows[0].status, "Completed");
    }

    #[tokio::test]
    async fn the_sales_list_is_newest_first() {
        let db = db::init_for_tests().await;
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
        seed_sale(&db, day.and_hms_opt(9, 0, 0).unwrap(), Decimal::new(1_000, 3), None, "Completed").await;
        let later = seed_sale(&db, day.and_hms_opt(17, 0, 0).unwrap(), Decimal::new(2_000, 3), None, "Completed").await;

        let rows = commands::list_sales_in(&db, &no_filter(), &page_one()).await.unwrap().rows;
        assert_eq!(rows.first().unwrap().id, later, "the newest sale is not first");
    }

    #[tokio::test]
    async fn a_sale_reports_one_stock_balance_per_distinct_item() {
        let db = db::init_for_tests().await;
        let first = seed_item(&db, "Widget").await;
        let second = seed_item(&db, "Gadget").await;
        seed_stock(&db, first, Decimal::new(10_000, 3)).await;
        seed_stock(&db, second, Decimal::new(5_000, 3)).await;

        // The same item twice, the way a cashier merging two scans produces.
        let view = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![
                    commands::CheckoutLine {
                        item_id: first,
                        quantity: Decimal::new(1_000, 3),
                        unit_price: Decimal::new(2_000, 3),
                        discount: None,
                    },
                    commands::CheckoutLine {
                        item_id: first,
                        quantity: Decimal::new(2_000, 3),
                        unit_price: Decimal::new(2_000, 3),
                        discount: None,
                    },
                    commands::CheckoutLine {
                        item_id: second,
                        quantity: Decimal::new(1_000, 3),
                        unit_price: Decimal::new(3_000, 3),
                        discount: None,
                    },
                ],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout");

        assert_eq!(view.lines.len(), 3, "both lines of the repeated item are kept");
        assert_eq!(
            view.stock_on_hand.len(),
            2,
            "the repeated item reports one balance, not one per line"
        );

        let reloaded = commands::get_sale_in(&db, view.sale.id).await.expect("reload the sale");
        assert_eq!(reloaded.sale.invoice_no, view.sale.invoice_no);
        assert_eq!(reloaded.lines.len(), 3);
        assert_eq!(
            reloaded
                .stock_on_hand
                .iter()
                .find(|o| o.item_id == first)
                .map(|o| o.quantity),
            Some(Decimal::new(7_000, 3)),
            "reloading a sale reports a different shelf than the sale left behind"
        );
    }

    #[tokio::test]
    async fn an_unknown_sale_is_not_found() {
        let db = db::init_for_tests().await;
        assert!(matches!(
            commands::get_sale_in(&db, 9999).await,
            Err(commands::CmdError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn a_split_payment_writes_one_row_per_tender() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        let view = sell_one_item(
            &db,
            item,
            Decimal::new(100_000, 3),
            None,
            Some(vec![
                commands::PaymentLine {
                    method: "Cash".into(),
                    amount: Decimal::new(60_000, 3),
                    reference: None,
                },
                commands::PaymentLine {
                    method: "Card".into(),
                    amount: Decimal::new(40_000, 3),
                    reference: Some("AUTH-9911".into()),
                },
            ]),
        )
        .await
        .expect("split checkout");

        assert_eq!(view.payments.len(), 2, "the tenders are not recorded");
        assert_eq!(
            view.sale.paid_total,
            Decimal::new(100_000, 3),
            "the paid figure comes from the tenders, not the input"
        );
        assert_eq!(
            view.sale.payment_method, "Cash + Card",
            "the header should name what happened, not \"Cash\" from the input"
        );

        let stored = commands::list_payments_in(&db, view.sale.id).await.unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(
            stored.iter().map(|p| p.amount).sum::<Decimal>(),
            view.sale.grand_total,
            "the tenders and the header disagree"
        );
    }

    #[tokio::test]
    async fn payments_above_the_total_are_refused() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        let err = sell_one_item(
            &db,
            item,
            Decimal::new(100_000, 3),
            None,
            Some(vec![commands::PaymentLine {
                method: "Cash".into(),
                amount: Decimal::new(120_000, 3),
                reference: None,
            }]),
        )
        .await
        .expect_err("the tenders exceed the sale");

        assert!(
            matches!(err, commands::CmdError::Validation(ref m) if m.contains("more than the sale total")),
            "unexpected error: {err}"
        );
        assert_eq!(
            sale::Entity::find().count(&db).await.unwrap(),
            0,
            "a refused payment left a sale behind"
        );
    }

    #[tokio::test]
    async fn a_zero_tender_is_refused() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        let err = sell_one_item(
            &db,
            item,
            Decimal::new(100_000, 3),
            None,
            Some(vec![commands::PaymentLine {
                method: "Cash".into(),
                amount: Decimal::ZERO,
                reference: None,
            }]),
        )
        .await
        .expect_err("a zero tender is not a tender");

        assert!(
            matches!(err, commands::CmdError::Validation(ref m) if m.contains("greater than zero")),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn a_part_paid_split_sale_is_allowed() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        // Less than the total is a customer paying part of it, which is legal and is
        // the same shape as a on-account sale.
        let view = sell_one_item(
            &db,
            item,
            Decimal::new(100_000, 3),
            None,
            Some(vec![commands::PaymentLine {
                method: "Cash".into(),
                amount: Decimal::new(40_000, 3),
                reference: None,
            }]),
        )
        .await
        .expect("part-paid checkout");

        assert_eq!(view.sale.paid_total, Decimal::new(40_000, 3));
        assert!(view.sale.paid_total < view.sale.grand_total);
    }

    #[tokio::test]
    async fn a_return_puts_the_stock_back() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;
        let (sale_id, line_id) = sell_one(&db, item, Decimal::new(2_000, 3), Decimal::new(25_000, 3)).await;
        assert_eq!(commands::stock_on_hand_in(&db, item).await.unwrap(), Decimal::new(8_000, 3));

        let mut input = return_line(line_id, Decimal::new(1_000, 3));
        input.sale_id = sale_id;
        let view = commands::create_return_in(&db, input, Some(7)).await.expect("return one");

        assert_eq!(
            view.refunded_total,
            Decimal::new(25_000, 3),
            "the refund is the line price times the returned quantity"
        );
        assert_eq!(
            commands::stock_on_hand_in(&db, item).await.unwrap(),
            Decimal::new(9_000, 3),
            "the shelf did not grow by what came back"
        );
        assert_eq!(view.returned_by, Some(7), "the return records who authorised it");
        assert!(view.return_no.starts_with("RET-"), "got {}", view.return_no);
    }

    #[tokio::test]
    async fn returning_more_than_was_sold_is_refused() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;
        let (sale_id, line_id) = sell_one(&db, item, Decimal::new(2_000, 3), Decimal::new(25_000, 3)).await;

        let mut input = return_line(line_id, Decimal::new(3_000, 3));
        input.sale_id = sale_id;
        let err = commands::create_return_in(&db, input, None).await.expect_err("more than was sold");

        assert!(matches!(err, commands::CmdError::Validation(_)), "unexpected error: {err}");
        assert_eq!(
            commands::stock_on_hand_in(&db, item).await.unwrap(),
            Decimal::new(8_000, 3),
            "a refused return still added stock back"
        );
    }

    #[tokio::test]
    async fn two_partial_returns_cannot_exceed_the_sold_quantity() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;
        let (sale_id, line_id) = sell_one(&db, item, Decimal::new(2_000, 3), Decimal::new(25_000, 3)).await;

        for quantity in [Decimal::new(1_500, 3), Decimal::new(500, 3)] {
            let mut input = return_line(line_id, quantity);
            input.sale_id = sale_id;
            commands::create_return_in(&db, input, None).await.expect("partial return");
        }
        // Both halves are back, so nothing is left.
        assert_eq!(
            commands::returned_quantity(&db, line_id).await.unwrap(),
            Decimal::new(2_000, 3)
        );

        let mut input = return_line(line_id, Decimal::new(1, 3));
        input.sale_id = sale_id;
        assert!(
            commands::create_return_in(&db, input, None).await.is_err(),
            "a third return hands back stock that was never sold"
        );
        assert_eq!(
            commands::stock_on_hand_in(&db, item).await.unwrap(),
            Decimal::new(10_000, 3),
            "the shelf is back to where it started"
        );
    }

    #[tokio::test]
    async fn a_line_from_another_sale_cannot_be_returned() {
        let db = db::init_for_tests().await;
        let first = seed_item(&db, "Widget").await;
        let second = seed_item(&db, "Gadget").await;
        seed_stock(&db, first, Decimal::new(10_000, 3)).await;
        seed_stock(&db, second, Decimal::new(10_000, 3)).await;
        let (sale_id, _) = sell_one(&db, first, Decimal::new(1_000, 3), Decimal::new(5_000, 3)).await;
        let (_, other_line) = sell_one(&db, second, Decimal::new(1_000, 3), Decimal::new(5_000, 3)).await;

        let mut input = return_line(other_line, Decimal::new(1_000, 3));
        input.sale_id = sale_id;
        assert!(
            commands::create_return_in(&db, input, None).await.is_err(),
            "a line from a different sale is returnable against this one"
        );
    }

    #[tokio::test]
    async fn a_draft_sale_cannot_be_returned_against() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;

        let draft = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id: item,
                    quantity: Decimal::new(1_000, 3),
                    unit_price: Decimal::new(5_000, 3),
                    discount: None,
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(false),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("a draft");

        let mut input = return_line(draft.lines[0].id, Decimal::new(1_000, 3));
        input.sale_id = draft.sale.id;
        assert!(
            commands::create_return_in(&db, input, None).await.is_err(),
            "a draft that was never paid is returnable"
        );
    }

    #[tokio::test]
    async fn an_unknown_return_reason_is_refused() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;
        let (sale_id, line_id) = sell_one(&db, item, Decimal::new(1_000, 3), Decimal::new(5_000, 3)).await;

        let mut input = return_line(line_id, Decimal::new(1_000, 3));
        input.sale_id = sale_id;
        input.reason = "customer was annoyed".into();

        assert!(
            matches!(
                commands::create_return_in(&db, input, None).await,
                Err(commands::CmdError::Validation(ref m)) if m.contains("closed")
            ),
            "free text was accepted, so returns cannot be grouped in a report"
        );
    }

    #[tokio::test]
    async fn returns_against_one_sale_accumulate() {
        let db = db::init_for_tests().await;
        let item = seed_item(&db, "Widget").await;
        seed_stock(&db, item, Decimal::new(10_000, 3)).await;
        let (sale_id, line_id) = sell_one(&db, item, Decimal::new(3_000, 3), Decimal::new(10_000, 3)).await;

        for quantity in [Decimal::new(1_000, 3), Decimal::new(500, 3)] {
            let mut input = return_line(line_id, quantity);
            input.sale_id = sale_id;
            commands::create_return_in(&db, input, None).await.expect("partial return");
        }

        let listed = commands::list_returns_in(&db, Some(sale_id)).await.unwrap();
        assert_eq!(listed.len(), 2, "both returns are listed against the sale");
        let refunded: Decimal = listed.iter().map(|r| r.refunded_total).sum();
        assert_eq!(refunded, Decimal::new(15_000, 3));
    }

    // -----------------------------------------------------------------------
    // Registers
    //
    // Reached through the `_in` forms with an explicit user id: the scope *is*
    // the user, so there is no session cell to read and nothing for parallel
    // tests to race on.
    // -----------------------------------------------------------------------

    fn open_input(opening: Decimal) -> commands::OpenRegisterInput {
        commands::OpenRegisterInput {
            opening_balance: opening,
            opening_details: None,
            note: None,
        }
    }

    async fn open_for(db: &DatabaseConnection, user_id: i32, opening: Decimal) -> i32 {
        commands::open_register_in(db, user_id, open_input(opening))
            .await
            .expect("open register")
            .id
    }

    #[tokio::test]
    async fn open_register_creates_an_open_shift() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let view = commands::open_register_in(&db, user, open_input(Decimal::new(100_000, 3)))
            .await
            .expect("open");
        assert_eq!(view.status, "Open");
        assert_eq!(view.opening_balance, Decimal::new(100_000, 3));
        assert!(view.closed_at.is_none());
        assert!(view.closing_balance.is_none());
        assert!(view.expected_balance.is_none());
    }

    #[tokio::test]
    async fn second_open_while_one_is_open_is_refused() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        open_for(&db, user, Decimal::ZERO).await;
        let err = commands::open_register_in(&db, user, open_input(Decimal::ZERO)).await;
        assert!(
            matches!(err, Err(commands::CmdError::Conflict(_))),
            "two open shifts for one user means sales land in the wrong window"
        );
    }

    #[tokio::test]
    async fn registers_are_scoped_to_the_user_who_opened_them() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let other = seed_user(&db).await;
        open_for(&db, user, Decimal::ZERO).await;
        // A second cashier on the same till is a different shift, not a conflict.
        open_for(&db, other, Decimal::ZERO).await;
        let mine = commands::current_register_in(&db, other).await.unwrap();
        assert!(mine.is_some());
    }

    #[tokio::test]
    async fn close_without_an_open_shift_is_refused() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let err = commands::close_register_in(
            &db,
            user,
            commands::CloseRegisterInput { closing_balance: Decimal::ZERO, note: None },
        )
        .await;
        assert!(matches!(err, Err(commands::CmdError::Conflict(_))));
    }

    #[tokio::test]
    async fn close_twice_is_refused() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        open_for(&db, user, Decimal::ZERO).await;
        let close = || {
            commands::close_register_in(
                &db,
                user,
                commands::CloseRegisterInput { closing_balance: Decimal::ZERO, note: None },
            )
        };
        close().await.expect("first close");
        assert!(
            matches!(close().await, Err(commands::CmdError::Conflict(_))),
            "closing twice would snapshot — and report — the same shift twice"
        );
    }

    #[tokio::test]
    async fn close_snapshots_expected_from_sales_in_window() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        // Yesterday's sale belongs to no open shift.
        seed_sale(&db, days_ago(1), dec(999), None, "Completed").await;

        open_for(&db, user, Decimal::new(100_000, 3)).await;
        sell_one_item(&db, cola, Decimal::new(50_000, 3), None, None)
            .await
            .expect("cash sale");

        let view = commands::close_register_in(
            &db,
            user,
            commands::CloseRegisterInput { closing_balance: Decimal::new(150_000, 3), note: None },
        )
        .await
        .expect("close");
        assert_eq!(view.status, "Closed");
        assert_eq!(view.expected_balance, Some(Decimal::new(150_000, 3)));
        assert_eq!(view.variance, Some(Decimal::ZERO));
    }

    #[tokio::test]
    async fn close_counts_card_tenders_outside_the_drawer() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        open_for(&db, user, Decimal::ZERO).await;

        sell_one_item(
            &db,
            cola,
            Decimal::new(100_000, 3),
            None,
            Some(vec![
                commands::PaymentLine { method: "Cash".into(), amount: Decimal::new(30_000, 3), reference: None },
                commands::PaymentLine { method: "Card".into(), amount: Decimal::new(70_000, 3), reference: None },
            ]),
        )
        .await
        .expect("split sale");

        let summary = commands::register_summary_in(&db, user).await.expect("summary").expect("open");
        assert_eq!(summary.cash_total, Decimal::new(30_000, 3));
        assert_eq!(summary.other_total, Decimal::new(70_000, 3));
        assert_eq!(summary.expected_balance, Decimal::new(30_000, 3));
    }

    #[tokio::test]
    async fn close_subtracts_refunds_and_adds_receipts() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        open_for(&db, user, Decimal::ZERO).await;

        let (sale_id, line_id) = sell_one(&db, cola, dec(2), Decimal::new(25_000, 3)).await;
        let mut input = return_line(line_id, dec(1));
        input.sale_id = sale_id;
        commands::create_return_in(&db, input, None).await.expect("return half");

        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        commands::record_customer_receipt_in(
            &db,
            customer,
            commands::ReceiveInput { amount: Decimal::new(10_000, 3), reference: None, paid_at: None },
        )
        .await
        .expect("receipt");

        let summary = commands::register_summary_in(&db, user).await.expect("summary").expect("open");
        assert_eq!(summary.refunded_total, Decimal::new(25_000, 3));
        assert_eq!(summary.receipts_total, Decimal::new(10_000, 3));
        // 50 cash in, 25 refunded out, 10 debt collected: 35 expected.
        assert_eq!(summary.expected_balance, Decimal::new(35_000, 3));
    }

    #[tokio::test]
    async fn opening_details_must_match_the_float() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        let err = commands::open_register_in(
            &db,
            user,
            commands::OpenRegisterInput {
                opening_balance: Decimal::new(100_000, 3),
                opening_details: Some(vec![commands::MethodTotal {
                    method: "Cash".into(),
                    amount: Decimal::new(90_000, 3),
                }]),
                note: None,
            },
        )
        .await;
        assert!(
            matches!(err, Err(commands::CmdError::Validation(_))),
            "a breakdown that does not add up is a typo, not a second figure"
        );
    }

    #[tokio::test]
    async fn negative_opening_is_refused() {
        let db = db::init_for_tests().await;
        let user = seed_user(&db).await;
        assert!(
            matches!(
                commands::open_register_in(&db, user, open_input(Decimal::new(-1, 3))).await,
                Err(commands::CmdError::Validation(_))
            )
        );
    }

    // -----------------------------------------------------------------------
    // Quotations
    // -----------------------------------------------------------------------

    fn quote_input(customer_id: i32, item_id: i32) -> commands::QuotationInput {
        commands::QuotationInput {
            customer_id,
            quoted_at: None,
            reference_no: None,
            discount_total: Some(Decimal::new(5_000, 3)),
            note: None,
            lines: vec![
                commands::QuotationLine {
                    item_id,
                    quantity: dec(2),
                    unit_price: Decimal::new(25_000, 3),
                    discount: Some(Decimal::new(1_000, 3)),
                },
                commands::QuotationLine {
                    item_id,
                    quantity: dec(1),
                    unit_price: Decimal::new(10_000, 3),
                    discount: None,
                },
            ],
        }
    }

    #[tokio::test]
    async fn create_quotation_derives_totals_and_number() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let widget = seed_item(&db, "Widget").await;

        let view = commands::create_quotation_in(&db, quote_input(customer, widget))
            .await
            .expect("create");
        // 2×25 − 1 discount, plus 1×10: subtotal 60, discounts 6, grand 54.
        assert_eq!(view.subtotal, Decimal::new(60_000, 3));
        assert_eq!(view.discount_total, Decimal::new(6_000, 3));
        assert_eq!(view.grand_total, Decimal::new(54_000, 3));
        assert!(view.quotation_no.starts_with("QT-"), "got {}", view.quotation_no);
        assert_eq!(view.customer_name.as_deref(), Some("Regular"));
        assert_eq!(view.lines.len(), 2);
        assert!(view.lines.iter().all(|l| l.item_name == "Widget"));
    }

    #[tokio::test]
    async fn quotation_requires_a_live_customer() {
        let db = db::init_for_tests().await;
        let widget = seed_item(&db, "Widget").await;
        let customer = seed_customer(&db, "Gone", Decimal::ZERO).await;
        let mut gone: customer::ActiveModel = customer::Entity::find_by_id(customer)
            .one(&db)
            .await
            .unwrap()
            .expect("seeded")
            .into();
        gone.del_status = Set("Deleted".into());
        gone.update(&db).await.unwrap();

        for id in [4242, customer] {
            let err = commands::create_quotation_in(&db, quote_input(id, widget)).await;
            assert!(
                matches!(err, Err(commands::CmdError::NotFound(_))),
                "an offer to nobody is a row nothing can collect on"
            );
        }
    }

    #[tokio::test]
    async fn quotation_rejects_an_unknown_item_without_leaving_a_header() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let mut input = quote_input(customer, 4242);
        input.lines[0].item_id = 4242;

        assert!(
            matches!(
                commands::create_quotation_in(&db, input).await,
                Err(commands::CmdError::NotFound(_))
            )
        );
        assert_eq!(
            quotation::Entity::find().count(&db).await.unwrap(),
            0,
            "the header survived the failed line write"
        );
    }

    #[tokio::test]
    async fn quotation_rejects_a_bad_date() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let widget = seed_item(&db, "Widget").await;
        let mut input = quote_input(customer, widget);
        input.quoted_at = Some("03/10/2026".into());
        assert!(
            matches!(
                commands::create_quotation_in(&db, input).await,
                Err(commands::CmdError::Validation(_))
            )
        );
    }

    #[tokio::test]
    async fn update_quotation_replaces_lines() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let widget = seed_item(&db, "Widget").await;
        let gadget = seed_item(&db, "Gadget").await;

        let created = commands::create_quotation_in(&db, quote_input(customer, widget))
            .await
            .expect("create");
        let mut input = quote_input(customer, gadget);
        input.discount_total = None;
        let updated = commands::update_quotation_in(&db, created.id, input)
            .await
            .expect("update");

        assert_eq!(updated.lines.len(), 2);
        assert!(updated.lines.iter().all(|l| l.item_name == "Gadget"));
        assert_eq!(updated.subtotal, Decimal::new(60_000, 3));
        assert_eq!(updated.grand_total, Decimal::new(59_000, 3));
        assert_eq!(
            quotation_detail::Entity::find().count(&db).await.unwrap(),
            2,
            "the old lines were appended to, not replaced"
        );
    }

    #[tokio::test]
    async fn delete_quotation_removes_header_and_lines() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let widget = seed_item(&db, "Widget").await;
        let created = commands::create_quotation_in(&db, quote_input(customer, widget))
            .await
            .expect("create");

        commands::delete_quotation_in(&db, created.id).await.expect("delete");
        assert_eq!(quotation::Entity::find().count(&db).await.unwrap(), 0);
        assert_eq!(quotation_detail::Entity::find().count(&db).await.unwrap(), 0);
        assert!(matches!(
            commands::delete_quotation_in(&db, created.id).await,
            Err(commands::CmdError::NotFound(_))
        ));
    }

    // -----------------------------------------------------------------------
    // Bookings
    // -----------------------------------------------------------------------

    fn at(days_ahead: i64, hour: u32, min: u32) -> String {
        let day = crate::migration::now().date() + chrono::Duration::days(days_ahead);
        format!("{}T{:02}:{:02}", day.format("%Y-%m-%d"), hour, min)
    }

    fn book_input(customer_id: i32) -> commands::BookingInput {
        commands::BookingInput {
            customer_id,
            service_seller_id: None,
            status: None,
            start_at: at(1, 10, 0),
            end_at: at(1, 11, 0),
            note: None,
        }
    }

    #[tokio::test]
    async fn create_booking_names_customer_and_seller() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let seller = seed_user(&db).await;
        let mut input = book_input(customer);
        input.service_seller_id = Some(seller);

        let view = commands::create_booking_in(&db, input, Some(seller)).await.expect("create");
        assert_eq!(view.status, "Booked");
        assert_eq!(view.customer_name.as_deref(), Some("Regular"));
        assert_eq!(view.service_seller_name.as_deref(), Some("Cashier"));
    }

    #[tokio::test]
    async fn booking_cannot_start_in_the_past() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let mut input = book_input(customer);
        input.start_at = at(-1, 10, 0);
        input.end_at = at(-1, 11, 0);
        assert!(
            matches!(
                commands::create_booking_in(&db, input, None).await,
                Err(commands::CmdError::Validation(_))
            )
        );
    }

    #[tokio::test]
    async fn booking_cannot_end_before_it_starts() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let mut input = book_input(customer);
        input.end_at = at(1, 9, 0);
        assert!(
            matches!(
                commands::create_booking_in(&db, input, None).await,
                Err(commands::CmdError::Validation(_))
            )
        );
    }

    #[tokio::test]
    async fn booking_rejects_an_unknown_status_or_party() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;

        let mut input = book_input(customer);
        input.status = Some("Someday".into());
        assert!(
            matches!(
                commands::create_booking_in(&db, input, None).await,
                Err(commands::CmdError::Validation(_))
            )
        );

        let input = book_input(4242);
        assert!(
            matches!(
                commands::create_booking_in(&db, input, None).await,
                Err(commands::CmdError::NotFound(_))
            )
        );

        let mut input = book_input(customer);
        input.service_seller_id = Some(4242);
        assert!(
            matches!(
                commands::create_booking_in(&db, input, None).await,
                Err(commands::CmdError::NotFound(_))
            )
        );
    }

    #[tokio::test]
    async fn update_booking_cannot_move_a_past_booking_further_back() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let view = commands::create_booking_in(&db, book_input(customer), None)
            .await
            .expect("create");

        // Backdate the row directly: create refuses the past, so only an edit path
        // or a database write can put one there.
        let past = days_ago(2);
        let mut am: booking::ActiveModel = booking::Entity::find_by_id(view.id)
            .one(&db)
            .await
            .unwrap()
            .expect("seeded")
            .into();
        am.start_at = Set(past);
        am.end_at = Set(past);
        am.update(&db).await.unwrap();

        let mut input = book_input(customer);
        input.start_at = at(-3, 10, 0);
        input.end_at = at(-3, 11, 0);
        assert!(
            matches!(
                commands::update_booking_in(&db, view.id, input).await,
                Err(commands::CmdError::Validation(_))
            )
        );

        // Forward is always fine, and so is the original slot.
        let mut input = book_input(customer);
        input.start_at = at(5, 10, 0);
        input.end_at = at(5, 11, 0);
        let moved = commands::update_booking_in(&db, view.id, input).await.expect("move forward");
        assert_eq!(moved.status, "Booked");
    }

    #[tokio::test]
    async fn delete_booking_soft_deletes() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let view = commands::create_booking_in(&db, book_input(customer), None)
            .await
            .expect("create");

        commands::delete_booking_in(&db, view.id).await.expect("delete");
        assert_eq!(booking::Entity::find().count(&db).await.unwrap(), 1);
        assert!(
            matches!(
                commands::delete_booking_in(&db, view.id).await,
                Err(commands::CmdError::NotFound(_))
            )
        );
        assert!(
            commands::list_bookings_in(&db, &commands::BookingFilter::default(), &page_one())
                .await
                .unwrap()
                .rows
                .is_empty()
        );
    }

    #[tokio::test]
    async fn list_bookings_is_soonest_first_and_filters_by_status() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let mut later = book_input(customer);
        later.start_at = at(3, 10, 0);
        later.end_at = at(3, 11, 0);
        commands::create_booking_in(&db, later, None).await.expect("later");
        let mut sooner = book_input(customer);
        sooner.status = Some("Waiting".into());
        commands::create_booking_in(&db, sooner, None).await.expect("sooner");

        let all = commands::list_bookings_in(&db, &commands::BookingFilter::default(), &page_one())
            .await
            .expect("list");
        assert_eq!(all.total, 2);
        assert_eq!(all.rows[0].status, "Waiting");

        let filtered = commands::list_bookings_in(
            &db,
            &commands::BookingFilter { status: Some("Booked".into()), ..Default::default() },
            &page_one(),
        )
        .await
        .expect("filtered");
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.rows[0].status, "Booked");
    }

    // -----------------------------------------------------------------------
    // Promotions
    // -----------------------------------------------------------------------

    fn promo_input(title: &str) -> commands::PromotionInput {
        commands::PromotionInput {
            title: title.into(),
            kind: "ItemPercent".into(),
            target_item_id: None,
            reward_item_id: None,
            percent: Some(Decimal::new(10, 0)),
            amount: None,
            min_total: None,
            buy_qty: None,
            get_qty: None,
            start_at: "2026-01-01".into(),
            end_at: "2026-12-31".into(),
        }
    }

    fn promo_item(item_id: i32) -> commands::PromotionInput {
        let mut input = promo_input("Ten off");
        input.target_item_id = Some(item_id);
        input
    }

    #[tokio::test]
    async fn create_promotion_stores_a_valid_rule() {
        let db = db::init_for_tests().await;
        let widget = seed_item(&db, "Widget").await;
        let row = commands::create_promotion_in(&db, promo_item(widget))
            .await
            .expect("create");
        assert_eq!(row.kind, "ItemPercent");
        assert_eq!(row.target_item_id, Some(widget));
    }

    #[tokio::test]
    async fn promotion_kind_and_columns_must_agree() {
        let db = db::init_for_tests().await;
        let widget = seed_item(&db, "Widget").await;

        // Percent kind carrying a fixed amount: refused, not silently ignored.
        let mut input = promo_item(widget);
        input.amount = Some(Decimal::new(5_000, 3));
        assert!(
            matches!(
                commands::create_promotion_in(&db, input).await,
                Err(commands::CmdError::Validation(_))
            )
        );

        // Item kind without an item: refused.
        let input = promo_input("Nowhere");
        assert!(
            matches!(
                commands::create_promotion_in(&db, input).await,
                Err(commands::CmdError::Validation(_))
            )
        );

        // Unknown kind: refused.
        let mut input = promo_item(widget);
        input.kind = "Mystery".into();
        assert!(
            matches!(
                commands::create_promotion_in(&db, input).await,
                Err(commands::CmdError::Validation(_))
            )
        );
    }

    #[tokio::test]
    async fn overlapping_promotions_on_one_item_are_refused() {
        let db = db::init_for_tests().await;
        let widget = seed_item(&db, "Widget").await;
        commands::create_promotion_in(&db, promo_item(widget))
            .await
            .expect("first");

        let mut input = promo_item(widget);
        input.title = "Overlapping".into();
        assert!(
            matches!(
                commands::create_promotion_in(&db, input).await,
                Err(commands::CmdError::Conflict(_))
            )
        );

        // A disjoint year is fine: the guard is about overlapping dates, not the item.
        let gadget = seed_item(&db, "Gadget").await;
        let mut input = promo_item(gadget);
        input.start_at = "2027-01-01".into();
        input.end_at = "2027-12-31".into();
        commands::create_promotion_in(&db, input).await.expect("disjoint");
    }

    #[tokio::test]
    async fn update_promotion_excludes_itself_from_the_overlap_check() {
        let db = db::init_for_tests().await;
        let widget = seed_item(&db, "Widget").await;
        let row = commands::create_promotion_in(&db, promo_item(widget))
            .await
            .expect("create");

        // Saving unchanged must not trip on itself.
        let mut input = promo_item(widget);
        input.title = "Ten off renamed".into();
        let updated = commands::update_promotion_in(&db, row.id, input).await.expect("update");
        assert_eq!(updated.title, "Ten off renamed");
    }

    #[tokio::test]
    async fn delete_promotion_removes_the_rule() {
        let db = db::init_for_tests().await;
        let widget = seed_item(&db, "Widget").await;
        let row = commands::create_promotion_in(&db, promo_item(widget))
            .await
            .expect("create");

        commands::delete_promotion_in(&db, row.id).await.expect("delete");
        assert!(
            matches!(
                commands::delete_promotion_in(&db, row.id).await,
                Err(commands::CmdError::NotFound(_))
            )
        );
    }

    // -----------------------------------------------------------------------
    // Promotion application
    //
    // Exercised through checkout, not the pure function: what matters is the money
    // on the stored rows, and that is only observable after a full write.
    // -----------------------------------------------------------------------

    fn percent_promo(item_id: i32, percent: i64) -> commands::PromotionInput {
        commands::PromotionInput {
            title: "Test percent".into(),
            kind: "ItemPercent".into(),
            target_item_id: Some(item_id),
            reward_item_id: None,
            percent: Some(Decimal::new(percent, 0)),
            amount: None,
            min_total: None,
            buy_qty: None,
            get_qty: None,
            start_at: "2020-01-01".into(),
            end_at: "2030-12-31".into(),
        }
    }

    #[tokio::test]
    async fn checkout_applies_an_item_percent_promo() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        commands::create_promotion_in(&db, percent_promo(cola, 10)).await.expect("promo");

        let view = sell_one_item(&db, cola, Decimal::new(100_000, 3), None, None)
            .await
            .expect("checkout");
        assert_eq!(view.lines[0].discount, Decimal::new(10_000, 3));
        assert_eq!(view.sale.grand_total, Decimal::new(90_000, 3));
    }

    #[tokio::test]
    async fn a_manual_discount_beats_a_promo_instead_of_stacking() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        commands::create_promotion_in(&db, percent_promo(cola, 50)).await.expect("promo");

        let view = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id: cola,
                    quantity: Decimal::new(1_000, 3),
                    unit_price: Decimal::new(100_000, 3),
                    discount: Some(Decimal::new(5_000, 3)),
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout");
        // The 5.000 manual discount stands; the 50% promo adds nothing.
        assert_eq!(view.lines[0].discount, Decimal::new(5_000, 3));
        assert_eq!(view.sale.grand_total, Decimal::new(95_000, 3));
    }

    #[tokio::test]
    async fn an_order_promo_needs_its_minimum_and_the_best_wins() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        // 5% and 20% both apply, so the best wins; the fixed 50 needs a 200
        // subtotal the 100 sale never reaches, so the minimum gate excludes it.
        for (title, kind, value, min) in [
            ("Five percent", "OrderPercent", Decimal::new(5, 0), Decimal::ZERO),
            ("Twenty percent", "OrderPercent", Decimal::new(20, 0), Decimal::ZERO),
            ("Fifty fixed", "OrderFixed", Decimal::new(50_000, 3), Decimal::new(200_000, 3)),
        ] {
            commands::create_promotion_in(
                &db,
                commands::PromotionInput {
                    title: title.into(),
                    kind: kind.into(),
                    target_item_id: None,
                    reward_item_id: None,
                    percent: if kind == "OrderPercent" { Some(value) } else { None },
                    amount: if kind == "OrderFixed" { Some(value) } else { None },
                    min_total: Some(min),
                    buy_qty: None,
                    get_qty: None,
                    start_at: "2020-01-01".into(),
                    end_at: "2030-12-31".into(),
                },
            )
            .await
            .expect("promo");
        }

        let view = sell_one_item(&db, cola, Decimal::new(100_000, 3), None, None)
            .await
            .expect("checkout");
        assert_eq!(view.sale.discount_total, Decimal::new(20_000, 3));
        assert_eq!(view.sale.grand_total, Decimal::new(80_000, 3));
    }

    #[tokio::test]
    async fn an_expired_promo_changes_nothing() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;
        let mut input = percent_promo(cola, 10);
        input.start_at = "2020-01-01".into();
        input.end_at = "2020-12-31".into();
        commands::create_promotion_in(&db, input).await.expect("promo");

        let view = sell_one_item(&db, cola, Decimal::new(100_000, 3), None, None)
            .await
            .expect("checkout");
        assert_eq!(view.lines[0].discount, Decimal::ZERO);
        assert_eq!(view.sale.grand_total, Decimal::new(100_000, 3));
    }

    #[tokio::test]
    async fn buy_get_discounts_the_reward_lines_in_the_cart() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        let chips = seed_item(&db, "Chips").await;
        seed_stock(&db, cola, dec(10)).await;
        seed_stock(&db, chips, dec(10)).await;
        commands::create_promotion_in(
            &db,
            commands::PromotionInput {
                title: "Buy 2 cola get 1 chips".into(),
                kind: "BuyGet".into(),
                target_item_id: Some(cola),
                reward_item_id: Some(chips),
                percent: None,
                amount: None,
                min_total: None,
                buy_qty: Some(Decimal::new(2_000, 3)),
                get_qty: Some(Decimal::new(1_000, 3)),
                start_at: "2020-01-01".into(),
                end_at: "2030-12-31".into(),
            },
        )
        .await
        .expect("promo");

        let view = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![
                    commands::CheckoutLine {
                        item_id: cola,
                        quantity: Decimal::new(2_000, 3),
                        unit_price: Decimal::new(50_000, 3),
                        discount: None,
                    },
                    commands::CheckoutLine {
                        item_id: chips,
                        quantity: Decimal::new(1_000, 3),
                        unit_price: Decimal::new(20_000, 3),
                        discount: None,
                    },
                ],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(true),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("checkout");
        let chips_line = view.lines.iter().find(|l| l.item_id == chips).expect("chips line");
        assert_eq!(chips_line.discount, Decimal::new(20_000, 3));
        assert_eq!(view.sale.grand_total, Decimal::new(100_000, 3));
    }

    #[tokio::test]
    async fn promote_applies_the_promos_live_at_promote_time() {
        let db = db::init_for_tests().await;
        let cola = seed_item(&db, "Cola").await;
        seed_stock(&db, cola, dec(10)).await;

        let draft = commands::checkout_in(
            &db,
            commands::CheckoutInput {
                lines: vec![commands::CheckoutLine {
                    item_id: cola,
                    quantity: Decimal::new(1_000, 3),
                    unit_price: Decimal::new(100_000, 3),
                    discount: None,
                }],
                discount_total: Some(Decimal::ZERO),
                tax_total: Some(Decimal::ZERO),
                paid_total: None,
                payment_method: Some("Cash".into()),
                note: None,
                promote: Some(false),
                customer_id: None,
                payments: None,
            },
        )
        .await
        .expect("park");
        assert_eq!(draft.lines[0].discount, Decimal::ZERO);

        commands::create_promotion_in(&db, percent_promo(cola, 10)).await.expect("promo");
        let view = commands::promote_draft_in(&db, draft.sale.id, None, None)
            .await
            .expect("promote");
        assert_eq!(view.lines[0].discount, Decimal::new(10_000, 3));
        assert_eq!(view.sale.grand_total, Decimal::new(90_000, 3));
    }

    #[tokio::test]
    async fn list_quotations_is_newest_first_with_customer_names() {
        let db = db::init_for_tests().await;
        let customer = seed_customer(&db, "Regular", Decimal::ZERO).await;
        let widget = seed_item(&db, "Widget").await;
        commands::create_quotation_in(&db, quote_input(customer, widget)).await.expect("first");
        let second = commands::create_quotation_in(&db, quote_input(customer, widget)).await.expect("second");

        let page = commands::list_quotations_in(&db, &page_one()).await.expect("list");
        assert_eq!(page.total, 2);
        assert_eq!(page.rows[0].id, second.id);
        assert!(page.rows.iter().all(|r| r.customer_name.as_deref() == Some("Regular")));
    }
}
