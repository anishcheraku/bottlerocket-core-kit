// src/uri_resolver.rs
use std::{convert::TryFrom, future::Future, pin::Pin, path::PathBuf};
use reqwest::Url;
use tokio::io::AsyncReadExt;

use crate::apply::{Error, Result};

/// Anything that can fetch itself as a UTF-8 `String`.
pub trait UriResolver {
    fn resolve(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>>;
}

/// “-” ⇒ stdin
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

impl UriResolver for StdinUri {
    fn resolve(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        Box::pin(async {
            let mut buf = String::new();
            tokio::io::stdin()
                .read_to_string(&mut buf)
                .await
                .map_err(|e| Error::StdinRead { source: e })?;
            Ok(buf)
        })
    }
}

/// file:// ⇒ local file
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

impl UriResolver for FileUri {
    fn resolve(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let path = self.path.clone();
        Box::pin(async move {
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| Error::FileRead { input_source: path.to_string_lossy().into_owned(), source: e })
        })
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

impl UriResolver for HttpUri {
    fn resolve(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let url = self.url.clone();
        Box::pin(async move {
            // <-- drop the `&` here, IntoUrl is implemented for Url, not &Url
            let resp = reqwest::get(url.clone())
                .await
                .map_err(|e| Error::Reqwest { uri: url.to_string(), method: "GET".to_string(), source: e })?
                .error_for_status()
                .map_err(|e| Error::Reqwest { uri: url.to_string(), method: "GET".to_string(), source: e })?;
            resp
                .text()
                .await
                .map_err(|e| Error::Reqwest { uri: url.to_string(), method: "GET".to_string(), source: e })
        })
    }
}


/// s3://bucket/key  (stub; requires aws-sdk-s3 to work)
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

impl UriResolver for S3Uri {
    fn resolve(&self) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        let uri = format!("s3://{}/{}", self.bucket, self.key);
        Box::pin(async move { Err(Error::Uri { input_source: uri, source: url::ParseError::RelativeUrlWithoutBase }) })
    }
}
