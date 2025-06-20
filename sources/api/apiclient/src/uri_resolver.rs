// src/uri_resolver.rs

use async_trait::async_trait;
use snafu::{ensure, ResultExt, OptionExt};
use std::convert::TryFrom;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

use reqwest::Url;
use crate::apply::{Error, Result};
use crate::apply::error::{
    FileReadSnafu, FileUriSnafu, ReqwestSnafu, StdinReadSnafu, S3UriMissingBucketSnafu, InvalidFileUriSnafu, InvalidHTTPUriSnafu, S3UriSchemeSnafu
};

/// Anything that can fetch itself as a UTF-8 `String`.
#[async_trait]
pub trait UriResolver {
    /// Fetches the contents of this URI as a `String`.
    async fn resolve(&self) -> Result<String>;
}

/// “-” → standard input
pub struct StdinUri;

impl TryFrom<&str> for StdinUri {
    type Error = ();

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        if s == "-" {
            Ok(StdinUri)
        } else {
            Err(())
        }
    }
}

#[async_trait]
impl UriResolver for StdinUri {
    async fn resolve(&self) -> Result<String> {
        let mut buf = String::new();
        tokio::io::stdin()
            .read_to_string(&mut buf)
            .await
            .context(StdinReadSnafu)?;
        Ok(buf)
    }
}

/// file:// URLs map to local filesystem paths
pub struct FileUri {
    path: PathBuf,
}

impl TryFrom<Url> for FileUri {
    type Error = Error;

    fn try_from(url: Url) -> std::result::Result<Self, Self::Error> {
        // only accept file://
        ensure!(
            url.scheme() == "file",
            InvalidFileUriSnafu { input_source: url.to_string() }
        );

        // convert to PathBuf or error
        let path = url
            .to_file_path()
            .ok()
            .context(FileUriSnafu { input_source: url.to_string() })?;

        Ok(FileUri { path })
    }
}

#[async_trait]
impl UriResolver for FileUri {
    async fn resolve(&self) -> Result<String> {
        tokio::fs::read_to_string(&self.path)
            .await
            .context(FileReadSnafu {
                input_source: self.path.to_string_lossy().into_owned(),
            })
    }
}

/// http:// or https:// URLs
pub struct HttpUri {
    url: Url,
}

impl TryFrom<Url> for HttpUri {
    type Error = Error;

    fn try_from(url: Url) -> std::result::Result<Self, Self::Error> {
        ensure!(
            url.scheme() == "http" || url.scheme() == "https",
            InvalidHTTPUriSnafu { input_source: url.to_string() }
        );
        Ok(HttpUri { url })
    }
}

#[async_trait]
impl UriResolver for HttpUri {
    async fn resolve(&self) -> Result<String> {
        let resp = reqwest::get(self.url.clone())
            .await
            .context(ReqwestSnafu {
                uri: self.url.to_string(),
                method: "GET".to_string(),
            })?
            .error_for_status()
            .context(ReqwestSnafu {
                uri: self.url.to_string(),
                method: "GET".to_string(),
            })?;

        resp.text()
            .await
            .context(ReqwestSnafu {
                uri: self.url.to_string(),
                method: "GET".to_string(),
            })
    }
}

/// s3://bucket/key URLs (stub; add aws-sdk-s3 later)
pub struct S3Uri {
    bucket: String,
    key: String,
}

impl TryFrom<Url> for S3Uri {
    type Error = Error;

    fn try_from(url: Url) -> std::result::Result<Self, Self::Error> {
        ensure!(
            url.scheme() == "s3",
            S3UriSchemeSnafu { input_source: url.to_string() }
        );
        let bucket = url
            .host_str()
            .context(S3UriMissingBucketSnafu { input_source: url.to_string() })?
            .to_string();
        let key = url.path().trim_start_matches('/').to_string();
        Ok(S3Uri { bucket, key })
    }
}

#[async_trait]
impl UriResolver for S3Uri {
    async fn resolve(&self) -> Result<String> {
        // still unimplemented
        Err(Error::Uri {
            input_source: format!("s3://{}/{}", self.bucket, self.key),
            source: url::ParseError::RelativeUrlWithoutBase,
        })
    }
}

