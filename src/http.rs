use std::path::{Path, PathBuf};
use std::sync::Arc;
use reqwest::Client;
use tokio::task::JoinSet;
use tokio::fs::{ File, create_dir_all, try_exists };
use tokio::io::AsyncWriteExt;

pub(crate) enum BaseUrl {
    External(String),
    MagentoMedia(String)
}

pub(crate) struct DownloadConfig {
    pub base_url: BaseUrl,
    pub base_path: String,
    pub user_agent: String,
    pub clients: u16,
}

pub(crate) trait DownloadProgress {
    fn completed(&mut self, image: String);

    fn error(&mut self, image: String);

    fn skipped(&mut self, image: String);
}

enum TaskResult {
    Success(String, Client),
    Skipped(String, Client),
    Error(String, Client)
}

pub(crate) struct HttpPool {
    pool: Vec<Client>,
    tasks: JoinSet<TaskResult>
}

impl HttpPool {
    pub(crate) fn new() -> Self {
        Self {
            pool: Vec::new(),
            tasks: JoinSet::new()
        }
    }

    pub(crate) async fn download(
        &mut self,
        images: impl Iterator<Item=String>,
        progress: &mut impl DownloadProgress,
        config: Arc<DownloadConfig>
    ) -> Result<(), anyhow::Error> {
        for image in images {
            if config.is_full((self.tasks.len() + self.pool.len()) as u16) {
                match self.tasks.join_next().await {
                    Some(Ok(TaskResult::Success(image, client))) => {
                        progress.completed(image);
                        self.pool.push(client);
                    },
                    Some(Ok(TaskResult::Error(image, client))) => {
                        progress.error(image);
                        self.pool.push(client);
                    },
                    Some(Ok(TaskResult::Skipped(image, client))) => {
                        progress.skipped(image);
                        self.pool.push(client);
                    },
                    _ => {}
                }
            }

            let client = match self.pool.pop() {
                Some(client) => client,
                None => Client::builder().user_agent(&config.user_agent).build()?
            };

            self.tasks.spawn({
                let config = config.clone();
                async move {
                    let image_path = Path::new(&image);
                    let download_url = config.image_url(&image_path);
                    let file_path = config.file_path(&image_path);

                    if try_exists(&file_path).await.unwrap_or(false) {
                        return TaskResult::Skipped(image, client)
                    }

                    let mut response = match client.get(download_url).send().await {
                        Ok(response) => response,
                        Err(_) => return TaskResult::Error(image, client)
                    };

                    let status = response.status();
                    if !status.is_success() {
                        return TaskResult::Error(format!("{image} - {}", status.as_str()), client);
                    }

                    match file_path.parent() {
                        Some(path) => if let Err(_) = create_dir_all(path).await {
                            return TaskResult::Error(image, client);
                        },
                        None => {}
                    }

                    let mut file = match File::create(&file_path).await {
                        Ok(file) => file,
                        Err(_) => return TaskResult::Error(image, client)
                    };

                    while let Ok(Some(chunk)) = response.chunk().await {
                        if let Err(_) = file.write_all(chunk.as_ref()).await {
                            return TaskResult::Error(image, client);
                        }
                    }

                    TaskResult::Success(image, client)
                }
            });
        }

        Ok(())
    }
}

fn relative_path(image: &Path) -> PathBuf {
    let mut path = PathBuf::from("catalog/product");
    path.push(image);
    path
}

impl DownloadConfig {
    fn is_full(&self, current_size: u16) -> bool {
        self.clients >= current_size
    }

    fn image_url(&self, image: &Path) -> String {
        let (base_url, path) = match &self.base_url {
            BaseUrl::External(base_url) => (base_url, PathBuf::from(image.file_name().unwrap_or_default())),
            BaseUrl::MagentoMedia(base_url) => (base_url, relative_path(image))
        };

        format!("{base_url}/{}", path.to_string_lossy())
    }

    fn file_path(&self, image: &Path) -> PathBuf {
        let mut path_buf = PathBuf::from(&self.base_path);
        path_buf.push(relative_path(image));
        path_buf
    }
}