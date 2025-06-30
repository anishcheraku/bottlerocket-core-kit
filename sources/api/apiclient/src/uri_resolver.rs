// src/uri_resolver.rs

use async_trait::async_trait;
use aws_sdk_secretsmanager as sm; 
use aws_config;
use snafu::{ensure, ResultExt, OptionExt};
use std::convert::TryFrom;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

use reqwest::Url;
use crate::apply::{Error, Result};
use crate::apply::error::{
    FileReadSnafu, FileUriSnafu, ReqwestSnafu, StdinReadSnafu, S3UriMissingBucketSnafu, InvalidFileUriSnafu, InvalidHTTPUriSnafu, S3UriSchemeSnafu, SecretsManagerUriSnafu, SecretsManagerGetSnafu, SecretsManagerStringMissingSnafu,
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

impl TryFrom<&Url> for FileUri {
    type Error = Error;

    fn try_from(url: &Url) -> std::result::Result<Self, Self::Error> {
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
        log::error!("tryfromS3");
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

/// secretsmanager://<secret_id>
pub struct SecretsManagerUri {
    secret_id: String,
}

impl TryFrom<&str> for SecretsManagerUri {
    type Error = Error;

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        const PREFIX: &str = "secretsmanager://";
        if let Some(id) = s.strip_prefix(PREFIX) {
            if id.is_empty() {
                return Err(Error::SecretsManagerUri { input_source: s.to_string() });
            }
            return Ok(SecretsManagerUri { secret_id: id.to_string() });
        }
        Err(Error::SecretsManagerUri { input_source: s.to_string() })
    }
}

#[async_trait]
impl UriResolver for SecretsManagerUri {
    async fn resolve(&self) -> Result<String> {
        use aws_config::{self, BehaviorVersion, Region};
        use aws_sdk_secretsmanager;

        // 1) load AWS config (region/account via env)
        let cfg = aws_config::load_from_env().await;
        let client = aws_sdk_secretsmanager::Client::new(&cfg);

        // 2) fetch the secret, propagating any SdkError into SecretsManagerGet
        let resp = client
            .get_secret_value()
            .secret_id(self.secret_id.clone())
            .send()
            .await
            .context(SecretsManagerGetSnafu { secret_id: self.secret_id.clone() })?;

        // 3) extract the string payload, or error if it was missing
        resp.secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu { secret_id: self.secret_id.clone() })
    }
}


