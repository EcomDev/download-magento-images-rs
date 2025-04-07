use reqwest::Client;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{create_dir_all, try_exists, File};
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;

#[derive(Debug, PartialEq)]
pub(crate) enum BaseUrl {
    External(String),
    MagentoMedia(String),
}

impl<T> From<T> for BaseUrl
where
    T: AsRef<str>,
{
    fn from(v: T) -> Self {
        let v = v.as_ref().trim_end_matches('/');

        if v.ends_with("/media") {
            return BaseUrl::MagentoMedia(v.to_string());
        }

        BaseUrl::External(v.to_string())
    }
}

pub(crate) struct DownloadConfig {
    pub base_url: BaseUrl,
    pub base_path: String,
    pub user_agent: String,
    pub clients: u16,
    pub verbose: bool,
}

pub(crate) trait DownloadProgress {
    fn completed(&mut self, image: String);

    fn error(&mut self, image: String);

    fn skipped(&mut self, image: String);
}

enum TaskResult {
    Success(String, Client),
    Skipped(String, Client),
    Error(String, Client),
}

pub(crate) struct HttpPool {
    pool: Vec<Client>,
    tasks: JoinSet<TaskResult>,
}

impl HttpPool {
    pub(crate) fn new() -> Self {
        Self {
            pool: Vec::new(),
            tasks: JoinSet::new(),
        }
    }

    pub(crate) async fn download(
        &mut self,
        images: impl Iterator<Item = String>,
        progress: &mut impl DownloadProgress,
        config: Arc<DownloadConfig>,
    ) -> Result<(), anyhow::Error> {
        for image in images {
            if config.is_full((self.tasks.len() + self.pool.len()) as u16) {
                match self.tasks.join_next().await {
                    Some(Ok(TaskResult::Success(image, client))) => {
                        progress.completed(image);
                        self.pool.push(client);
                    }
                    Some(Ok(TaskResult::Error(image, client))) => {
                        progress.error(image);
                        self.pool.push(client);
                    }
                    Some(Ok(TaskResult::Skipped(image, client))) => {
                        progress.skipped(image);
                        self.pool.push(client);
                    }
                    _ => {}
                }
            }

            let client = match self.pool.pop() {
                Some(client) => client,
                None => Client::builder().user_agent(&config.user_agent).build()?,
            };

            self.tasks.spawn({
                let config = config.clone();
                async move {
                    let image_path = Path::new(&image);
                    let download_url = config.image_url(image_path);
                    let file_path = config.file_path(image_path);

                    if try_exists(&file_path).await.unwrap_or(false) {
                        return TaskResult::Skipped(image, client);
                    }

                    if config.verbose {
                        println!("Attempting to download from URL: {}", download_url);
                    }
                    let mut response = match client.get(download_url.clone()).send().await {
                        Ok(response) => response,
                        Err(e) => return TaskResult::Error(format!("{image} - Network error: {}", e), client),
                    };

                    let status = response.status();
                    if config.verbose {
                        println!("Response status: {}", status);
                    }
                    if !status.is_success() {
                        return TaskResult::Error(format!("{image} - Status: {}", status.as_str()), client);
                    }

                    if config.verbose {
                        println!("Saving to file path: {}", file_path.display());
                    }
                    if let Some(path) = file_path.parent() {
                        match create_dir_all(path).await {
                            Ok(_) => {
                                if config.verbose {
                                    println!("Created directory: {}", path.display());
                                }
                            },
                            Err(e) => return TaskResult::Error(format!("{image} - Directory creation error: {}", e), client),
                        }
                    }

                    let mut file = match File::create(&file_path).await {
                        Ok(file) => file,
                        Err(e) => return TaskResult::Error(format!("{image} - File creation error: {}", e), client),
                    };

                    let mut total_bytes = 0;
                    while let Ok(Some(chunk)) = response.chunk().await {
                        total_bytes += chunk.len();
                        match file.write_all(chunk.as_ref()).await {
                            Ok(_) => {},
                            Err(e) => return TaskResult::Error(format!("{image} - File write error: {}", e), client),
                        }
                    }
                    if config.verbose {
                        println!("Downloaded {} bytes for {}", total_bytes, image);
                    }

                    TaskResult::Success(image, client)
                }
            });
        }

        Ok(())
    }
}


impl DownloadConfig {
    fn is_full(&self, current_size: u16) -> bool {
        self.clients >= current_size
    }

    fn image_url(&self, image: &Path) -> String {
        match &self.base_url {
            BaseUrl::External(base_url) => {
                let path = PathBuf::from(image.file_name().unwrap_or_default());
                format!("{base_url}/{}", path.to_string_lossy())
            },
            BaseUrl::MagentoMedia(base_url) => {
                // For Magento media URLs, we need to ensure we have the correct path structure
                let binding = image.to_string_lossy();
                let file_name_str = binding.trim_start_matches('/');

                // Extract just the filename part
                let file_name = if let Some(name) = image.file_name() {
                    name.to_string_lossy().to_string()
                } else {
                    // If we can't get the filename, use the whole path
                    file_name_str.to_string()
                };

                // Create the Magento-style URL path
                if file_name.len() >= 2 {
                    let first_char = &file_name[0..1];
                    let second_char = &file_name[1..2];
                    format!("{base_url}/catalog/product/{}/{}/{}", first_char, second_char, file_name)
                } else if file_name.len() == 1 {
                    let first_char = &file_name[0..1];
                    format!("{base_url}/catalog/product/{}/{}", first_char, file_name)
                } else {
                    format!("{base_url}/catalog/product/{}", file_name)
                }
            },
        }
    }

    fn file_path(&self, image: &Path) -> PathBuf {
        // Create a path relative to the current directory, not an absolute path
        let mut path_buf = PathBuf::from(&self.base_path);

        // Convert the image path to a string and remove any leading slashes
        let binding = image.to_string_lossy();
        let file_name_str = binding.trim_start_matches('/');

        // Extract just the filename part
        let file_name = if let Some(name) = image.file_name() {
            name.to_string_lossy().to_string()
        } else {
            // If we can't get the filename, use the whole path
            file_name_str.to_string()
        };

        // Create the Magento-style directory structure
        path_buf.push("catalog/product");

        if file_name.len() >= 1 {
            let first_char = &file_name[0..1];
            path_buf.push(first_char);

            if file_name.len() >= 2 {
                let second_char = &file_name[1..2];
                path_buf.push(second_char);
            }
        }

        // Add the filename itself
        path_buf.push(&file_name);

        path_buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_base_url_from_string_as_relative_path() {
        let base_url: BaseUrl = "http://some-magento.com/media/".into();

        assert_eq!(
            BaseUrl::MagentoMedia("http://some-magento.com/media".into()),
            base_url
        );
    }

    #[test]
    fn creates_base_url_as_external_when_no_media_path_exists() {
        let base_url: BaseUrl = "http://some-magento.com/test-folder/".into();

        assert_eq!(
            BaseUrl::External("http://some-magento.com/test-folder".into()),
            base_url
        );
    }
}
