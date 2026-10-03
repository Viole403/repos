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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    //! Migration smoke test against a real in-memory SQLite database.
    //!
    //! This is the only automated check in the repo so far. It proves the schema
    //! applies cleanly and that the soft-delete filter behaves — the two things most
    //! likely to break silently.
    use super::*;
    use crate::entities::catalog::{item, unit};
    use sea_orm::ActiveValue::Set;
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

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
    }
}
