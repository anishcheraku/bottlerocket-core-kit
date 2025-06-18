use async_trait::async_trait;
use std::{convert::TryFrom, path::PathBuf};
use reqwest::Url;
use tokio::io::AsyncReadExt;

use crate::apply::{Error, Result};

/// Anything that can fetch itself as a UTF-8 String.
#[async_trait]
pub trait UriResolver {
    /// Fetches the contents and returns as a String.
    async fn resolve(&self) -> Result<String>;
}

/// "-" → stdin
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
            .map_err(|e| Error::StdinRead { source: e })?;
        Ok(buf)
    }
}

/// file:// → local file
pub struct FileUri {
    path: PathBuf,
}

impl TryFrom<Url> for FileUri {
    type Error = Error;

    fn try_from(url: Url) -> std::result::Result<Self, Self::Error> {
        if url.scheme() != "file" {
            return Err(Error::Uri { input_source: url.to_string(), source: url::ParseError::RelativeUrlWithoutBase });
        }
        let path = url
            .to_file_path()
            .map_err(|_| Error::FileUri { input_source: url.to_string() })?;
        Ok(FileUri { path })
    }
}

#[async_trait]
impl UriResolver for FileUri {
    async fn resolve(&self) -> Result<String> {
        let content = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| Error::FileRead { input_source: self.path.to_string_lossy().into_owned(), source: e })?;
        Ok(content)
    }
}

/// http:// or https://
pub struct HttpUri {
    url: Url,
}

impl TryFrom<Url> for HttpUri {
    type Error = Error;

    fn try_from(url: Url) -> std::result::Result<Self, Self::Error> {
        match url.scheme() {
            "http" | "https" => Ok(HttpUri { url }),
            _ => Err(Error::Uri { input_source: url.to_string(), source: url::ParseError::RelativeUrlWithoutBase }),
        }
    }
}

#[async_trait]
impl UriResolver for HttpUri {
    async fn resolve(&self) -> Result<String> {
        let resp = reqwest::get(self.url.clone())
            .await
            .map_err(|e| Error::Reqwest { uri: self.url.to_string(), method: "GET".to_string(), source: e })?
            .error_for_status()
            .map_err(|e| Error::Reqwest { uri: self.url.to_string(), method: "GET".to_string(), source: e })?;
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Reqwest { uri: self.url.to_string(), method: "GET".to_string(), source: e })?;
        Ok(text)
    }
}

/// s3://bucket/key (stub; requires aws-sdk-s3)
pub struct S3Uri {
    bucket: String,
    key: String,
}

impl TryFrom<Url> for S3Uri {
    type Error = Error;

    fn try_from(url: Url) -> std::result::Result<Self, Self::Error> {
        if url.scheme() != "s3" {
            return Err(Error::Uri { input_source: url.to_string(), source: url::ParseError::RelativeUrlWithoutBase });
        }
        let bucket = url
            .host_str()
            .ok_or_else(|| Error::Uri { input_source: url.to_string(), source: url::ParseError::EmptyHost })?
            .to_string();
        let key = url.path().trim_start_matches('/').to_string();
        Ok(S3Uri { bucket, key })
    }
}

#[async_trait]
impl UriResolver for S3Uri {
    async fn resolve(&self) -> Result<String> {
        // TODO: implement using aws-sdk-s3
        Err(Error::Uri { input_source: format!("s3://{}/{}", self.bucket, self.key), source: url::ParseError::RelativeUrlWithoutBase })
    }
}
