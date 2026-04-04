use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use crate::error::DataError;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    Url(String),
    LocalFile(PathBuf),
}

impl DataSource {
    fn file_name(&self) -> Result<String, DownloadError> {
        match self {
            Self::Url(url) => {
                let name = url
                    .rsplit('/')
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| DownloadError::UnsupportedSource(url.clone()))?;
                Ok(name.to_string())
            }
            Self::LocalFile(path) => path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string())
                .ok_or_else(|| DownloadError::UnsupportedSource(path.display().to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadConfig {
    pub output_dir: PathBuf,
    pub timeout_secs: u64,
    pub overwrite: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("data/fetched"),
            timeout_secs: 30,
            overwrite: false,
        }
    }
}

pub struct DataDownloader {
    config: DownloadConfig,
    client: Client,
}

impl DataDownloader {
    pub fn new(config: DownloadConfig) -> Result<Self, DownloadError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|err| DownloadError::Http(err.to_string()))?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &DownloadConfig {
        &self.config
    }

    pub fn download(&self, source: &DataSource) -> Result<PathBuf, DownloadError> {
        fs::create_dir_all(&self.config.output_dir)?;
        let destination = self.config.output_dir.join(source.file_name()?);

        if destination.exists() && !self.config.overwrite {
            return Ok(destination);
        }

        match source {
            DataSource::Url(url) => {
                info!("Downloading {} -> {}", url, destination.display());
                let response = self
                    .client
                    .get(url)
                    .send()
                    .map_err(|err| DownloadError::Http(err.to_string()))?
                    .error_for_status()
                    .map_err(|err| DownloadError::Http(err.to_string()))?;
                let bytes = response
                    .bytes()
                    .map_err(|err| DownloadError::Http(err.to_string()))?;
                fs::write(&destination, &bytes)?;
                Ok(destination)
            }
            DataSource::LocalFile(path) => {
                info!("Copying {} -> {}", path.display(), destination.display());
                fs::copy(path, &destination)?;
                Ok(destination)
            }
        }
    }
}

pub fn quick_download(
    source: DataSource,
    output_dir: impl AsRef<Path>,
) -> Result<PathBuf, DataError> {
    let downloader = DataDownloader::new(DownloadConfig {
        output_dir: output_dir.as_ref().to_path_buf(),
        ..DownloadConfig::default()
    })
    .map_err(|err| DataError::Download(err.to_string()))?;

    downloader
        .download(&source)
        .map_err(|err| DataError::Download(err.to_string()))
}
