// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod commands;
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
    use crate::entities::catalog::{item, unit};
    use crate::entities::sales::stock_movement::MovementType;
    use crate::entities::sales::{sale, sale_detail, stock_movement};
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
        let now = chrono::Utc::now();
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

    // -----------------------------------------------------------------------
    // Sales + the stock ledger
    //
    // `checkout` is reached through its `_in` form: the `#[tauri::command]` shell
    // pulls the process-wide connection that only exists inside the Tauri window,
    // while the logic takes one as an argument and so works against a throwaway
    // in-memory database.
    // -----------------------------------------------------------------------

    /// An item with a code, since `items.code` is the only unique column.
    async fn seed_item(db: &DatabaseConnection, name: &str) -> i32 {
        let now = chrono::Utc::now();
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
            created_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed opening balance");
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

        let now = chrono::Utc::now();
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
    }
}
