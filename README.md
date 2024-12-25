# Media Downloader Tool

The Media Downloader Tool is a Rust-based application for downloading media files from a remote source while utilizing batched processing and high concurrency. This tool supports downloading large volumes of media efficiently and includes progress tracking.

## Features

- **Batch Processing:** Efficiently fetches and processes media in configurable batches.
- **Concurrency Control:** Controls the number of concurrent download clients.
- **Progress Tracking:** Displays a progress bar for the media download process.
- **Error Handling:** Reports failed, skipped, and completed downloads.
- **Database Integration:** Reads media data from a MySQL database table.

## Prerequisites

Before starting, ensure you have the following installed on your machine:

- [Rust](https://www.rust-lang.org/tools/install) (version `1.83.0` or later is recommended)
- Magento database with the necessary table (`catalog_product_entity_media_gallery`)

## Installation


1. Clone this repository:
   ```bash
   git clone <repository-url>
   cd <repository-directory>
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Set up your MySQL database connection and ensure the `catalog_product_entity_media_gallery` table is populated with entries.

## Usage

Run the tool with the following command:

```bash
Usage: ecomdev-download-magento-images [OPTIONS] <BASE_URL>

Arguments:
  <BASE_URL>  Base URL for media download

Options:
  -p, --base-path <BASE_PATH>        Directory path [default: pub/media]
  -u, --user-agent <USER_AGENT>      User agent [default: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36 Edg/125.0.0.0"]
  -c, --max-clients <MAX_CLIENTS>    Max number of clients to create for downloading [default: 100]
  -b, --batch-size <BATCH_SIZE>      Max number of items to fetch per batch [default: 10000]
  -d, --database-url <DATABASE_URL>  Database URL to use of connection [default: mysql://magento:magento@localhost/magento]
  -h, --help                         Print help
```

## Development

To develop or test this tool, follow these steps:

1. Ensure `sqlx` knows about your database schema by running:
   ```bash
   export DATABASE_URL=mysql://user:password@localhost/db
   cargo sqlx prepare -- --lib
   ```

2. Run in development mode:
   ```bash
   cargo run -- [OPTIONS]
   ```

3. Test the application with mock data or a test database.

## Contributing

Contributions are welcome! Please follow these steps:

1. Fork this repository.
2. Create a branch with your feature or fix:
   ```bash
   git checkout -b feature/my-feature
   ```
3. Commit your changes and push your branch:
   ```bash
   git push origin feature/my-feature
   ```
4. Open a pull request for review.

## License

This project is licensed under the MIT License. See the `LICENSE` file for details.