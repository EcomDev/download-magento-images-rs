mod http;

use std::sync::Arc;
use clap::Parser;
use indicatif::ProgressBar;
use sqlx::{Connection, MySqlConnection};
use crate::http::{BaseUrl, DownloadConfig, DownloadProgress, HttpPool};

#[derive(Parser)]
struct Options {
    /// Base URL for media download
    base_url: String,
    /// Directory path
    #[arg(short = 'p', long, default_value = "pub/media")]
    base_path: String,

    /// User agent
    #[arg(short = 'u', long, default_value = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 Edg/125.0.0.0")]
    user_agent: String,

    /// Max number of clients to create for downloading
    #[arg(short = 'c', long, default_value_t = 100)]
    max_clients: u16,

    /// Max number of items to fetch per batch
    #[arg(short = 'b', long, default_value_t = 10000)]
    batch_size: u16,

    #[arg(short = 'd', long, default_value = "mysql://magento:magento@localhost/magento")]
    /// Database URL to use of connection
    database_url: String,
}


async fn total(connection: &mut MySqlConnection) -> sqlx::Result<u64> {

    #[derive(sqlx::Type)]
    struct Total {
        total: i64
    }

    let query = sqlx::query_as!(
        Total,
        "SELECT COUNT(*) as total FROM catalog_product_entity_media_gallery"
    );

    Ok(query.fetch_one(connection).await?.total as u64)
}

async fn ranges(connection: &mut MySqlConnection, batch_size: u16) -> sqlx::Result<Vec<(u64, u64)>>  {

    #[derive(sqlx::Type)]
    struct MinMax { min: i64, max: i64 }

    let query = sqlx::query_as!(
        MinMax,
        "SELECT MIN(value_id) as `min!`, MAX(value_id) as `max!` FROM catalog_product_entity_media_gallery GROUP BY CEIL(value_id / ?)",
        batch_size
    );

    Ok(query.fetch_all(connection).await?.into_iter().map(|item| (item.min as u64, item.max as u64)).collect())
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
    let mut progress_bar = ProgressBar::new(total(&mut connection).await?);

    let download_config = Arc::new(DownloadConfig {
        base_url: BaseUrl::External(options.base_url),
        base_path: options.base_path,
        user_agent: options.user_agent,
        clients: options.max_clients,
    });

    struct Image {
        value: String
    }

    for (min, max) in ranges(&mut connection, options.batch_size).await? {
        let query = sqlx::query_as!(
            Image,
            "SELECT value as `value!` FROM catalog_product_entity_media_gallery WHERE value_id BETWEEN ? AND ?",
            min,
            max
        );

        let images = query.fetch_all(&mut connection).await?.into_iter().map(|v| v.value);
        http.download(images, &mut progress_bar, download_config.clone()).await?
    }

    progress_bar.finish();

    Ok(())
}
