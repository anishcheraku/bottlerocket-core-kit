// src/uri_resolver.rs
use std::future::Future;
use std::pin::Pin;

use reqwest::Url;
use snafu::{OptionExt, ResultExt};
use tokio::io::AsyncReadExt;

use crate::apply::{error, Result};

/// Trait for resolving different URI‐style inputs.
pub trait UriResolver {
    /// Can this resolver handle the given input string?
    fn can_resolve(&self, uri: &str) -> bool;

    /// Fetches the contents of `uri` as a `String`.
    fn resolve<'a>(
        &self,
        uri: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

/// Resolver for reading from stdin when the input is `"-"`.
pub struct StdinResolver;

impl UriResolver for StdinResolver {
    fn can_resolve(&self, uri: &str) -> bool {
        uri == "-"
    }

    fn resolve<'a>(
        &self,
        _uri: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let mut output = String::new();
            tokio::io::stdin()
                .read_to_string(&mut output)
                .await
                .context(error::StdinReadSnafu)?;
            Ok(output)
        })
    }
}

/// Resolver for `file://` URIs.
pub struct FileResolver;

impl UriResolver for FileResolver {
    fn can_resolve(&self, uri: &str) -> bool {
        match Url::parse(uri) {
            Ok(parsed) => parsed.scheme() == "file",
            Err(_) => false,
        }
    }

    fn resolve<'a>(
        &self,
        uri: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let parsed = Url::parse(uri).context(error::UriSnafu {
                input_source: uri.to_string(),
            })?;
            let path = parsed
                .to_file_path()
                .ok()
                .context(error::FileUriSnafu {
                    input_source: uri.to_string(),
                })?;
            tokio::fs::read_to_string(path)
                .await
                .context(error::FileReadSnafu {
                    input_source: uri.to_string(),
                })
        })
    }
}

/// Resolver for `http://` and `https://` URIs.
pub struct HttpResolver;

impl UriResolver for HttpResolver {
    fn can_resolve(&self, uri: &str) -> bool {
        match Url::parse(uri) {
            Ok(parsed) => matches!(parsed.scheme(), "http" | "https"),
            Err(_) => false,
        }
    }

    fn resolve<'a>(
        &self,
        uri: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            // Validate URI
            let parsed = Url::parse(uri).context(error::UriSnafu {
                input_source: uri.to_string(),
            })?;
            // Perform GET
            let resp = reqwest::get(parsed)
                .await
                .context(error::ReqwestSnafu {
                    uri: uri.to_string(),
                    method: "GET".to_string(),
                })?;
            // Check status
            let resp = resp.error_for_status().context(error::ReqwestSnafu {
                uri: uri.to_string(),
                method: "GET".to_string(),
            })?;
            // Read body
            resp.text()
                .await
                .context(error::ReqwestSnafu {
                    uri: uri.to_string(),
                    method: "GET".to_string(),
                })
        })
    }
}
