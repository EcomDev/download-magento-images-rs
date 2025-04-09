mod http;

use crate::http::{DownloadConfig, DownloadProgress, HttpPool};
use clap::Parser;
use indicatif::ProgressBar;
use sqlx::{Connection, MySqlConnection, Row};
use std::sync::Arc;

#[derive(Parser)]
struct Options {
    /// Base URL for media download
    base_url: String,
    /// Directory path
    #[arg(short = 'p', long, default_value = "pub/media")]
    base_path: String,

    /// User agent
    #[arg(
        short = 'u',
        long,
        default_value = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 Edg/125.0.0.0"
    )]
    user_agent: String,

    /// Max number of clients to create for downloading
    #[arg(short = 'c', long, default_value_t = 100)]
    max_clients: u16,

    /// Max number of items to fetch per batch
    #[arg(short = 'b', long, default_value_t = 10000)]
    batch_size: u16,

    /// Enable verbose output
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    /// Download category images instead of product images
    #[arg(short = 'g', long, default_value_t = false)]
    category_images: bool,

    #[arg(
        short = 'd',
        long,
        default_value = "mysql://magento:magento@localhost/magento"
    )]
    /// Database URL to use of connection
    database_url: String,
}

async fn total(connection: &mut MySqlConnection, category_images: bool) -> sqlx::Result<u64> {
    if category_images {
        // First get the attribute_id for category images
        let attribute_id = get_category_image_attribute_id(connection).await?;

        let row = sqlx::query(
            "SELECT COUNT(*) as total FROM catalog_category_entity_varchar WHERE attribute_id = ? AND value IS NOT NULL AND value != ''"
        )
        .bind(attribute_id)
        .fetch_one(connection)
        .await?;

        let count = row.get::<i64, _>("total") as u64;
        println!("Found {} category images", count);
        Ok(count)
    } else {
        let row = sqlx::query(
            "SELECT COUNT(*) as total FROM catalog_product_entity_media_gallery"
        )
        .fetch_one(connection)
        .await?;

        let count = row.get::<i64, _>("total") as u64;
        println!("Found {} product images", count);
        Ok(count)
    }
}

async fn get_category_image_attribute_id(connection: &mut MySqlConnection) -> sqlx::Result<u16> {
    println!("Fetching category image attribute ID...");
    let row = sqlx::query(
        "SELECT attribute_id FROM eav_attribute WHERE attribute_code = 'image' AND entity_type_id = 3"
    )
    .fetch_one(connection)
    .await?;

    let attribute_id = row.get::<u16, _>("attribute_id");
    println!("Found category image attribute ID: {}", attribute_id);

    Ok(attribute_id)
}

async fn ranges(
    connection: &mut MySqlConnection,
    batch_size: u16,
    category_images: bool,
) -> sqlx::Result<Vec<(u64, u64)>> {
    if category_images {
        // For category images, we'll use the value_id from catalog_category_entity_varchar
        // First get the attribute_id for category images
        let attribute_id = get_category_image_attribute_id(connection).await?;

        let rows = sqlx::query(
            "SELECT MIN(value_id) as min, MAX(value_id) as max FROM catalog_category_entity_varchar
             WHERE attribute_id = ? AND value IS NOT NULL AND value != ''
             GROUP BY CEIL(value_id / ?)"
        )
        .bind(attribute_id)
        .bind(batch_size)
        .fetch_all(connection)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| (row.get::<i32, _>("min") as u64, row.get::<i32, _>("max") as u64))
            .collect())
    } else {
        // For product images, use the original query
        let rows = sqlx::query(
            "SELECT MIN(value_id) as min, MAX(value_id) as max FROM catalog_product_entity_media_gallery GROUP BY CEIL(value_id / ?)"
        )
        .bind(batch_size)
        .fetch_all(connection)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| (row.get::<u32, _>("min") as u64, row.get::<u32, _>("max") as u64))
            .collect())
    }
}

impl DownloadProgress for ProgressBar {
    fn completed(&mut self, image: String) {
        self.inc(1);
        self.println(format!("Completed downloading {image}"))
    }

    fn error(&mut self, image: String) {
        self.inc(1);
        self.println(format!("Failed to download: {image}"));
    }

    fn skipped(&mut self, image: String) {
        self.inc(1);
        self.println(format!("Skipped as file exists: {image}"));
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let options = Options::try_parse()?;
    let mut connection = MySqlConnection::connect(&options.database_url).await?;
    let mut http = HttpPool::new();
    let mut progress_bar = ProgressBar::new(total(&mut connection, options.category_images).await?);

    let download_config = Arc::new(DownloadConfig {
        base_url: options.base_url.into(),
        base_path: options.base_path,
        user_agent: options.user_agent,
        clients: options.max_clients,
        verbose: options.verbose,
        is_category: options.category_images,
    });

    for (min, max) in ranges(&mut connection, options.batch_size, options.category_images).await? {
        let images = if options.category_images {
            // Get the attribute_id for category images
            let attribute_id = get_category_image_attribute_id(&mut connection).await?;

            // Query for category images
            println!("Fetching category images with attribute_id {} between value_id {} and {}", attribute_id, min, max);
            let rows = sqlx::query(
                "SELECT value FROM catalog_category_entity_varchar
                 WHERE attribute_id = ? AND value_id BETWEEN ? AND ?
                 AND value IS NOT NULL AND value != ''"
            )
            .bind(attribute_id)
            .bind(min)
            .bind(max)
            .fetch_all(&mut connection)
            .await?;

            println!("Found {} category images in this batch", rows.len());
            if !rows.is_empty() {
                println!("Sample image path: {}", rows[0].get::<String, _>("value"));
            }

            rows
                .into_iter()
                .map(|row| row.get::<String, _>("value"))
                .collect::<Vec<String>>()
        } else {
            // Query for product images
            let rows = sqlx::query(
                "SELECT value FROM catalog_product_entity_media_gallery WHERE value_id BETWEEN ? AND ?"
            )
            .bind(min)
            .bind(max)
            .fetch_all(&mut connection)
            .await?;

            println!("Found {} product images in this batch", rows.len());
            if !rows.is_empty() {
                println!("Sample image path: {}", rows[0].get::<String, _>("value"));
            }

            rows
                .into_iter()
                .map(|row| row.get::<String, _>("value"))
                .collect::<Vec<String>>()
        };

        http.download(images.into_iter(), &mut progress_bar, download_config.clone())
            .await?
    }

    progress_bar.finish();

    Ok(())
}
