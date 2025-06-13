// src/uri_resolver.rs
use std::{fs, io::Read};
use reqwest::blocking;
use reqwest::Url;
use snafu::{OptionExt, ResultExt};

use crate::apply::{error, Result};

/// Trait for resolving different URI‐style inputs.
pub trait UriResolver {
    /// Can this resolver handle the given input string?
    fn can_resolve(&self, uri: &str) -> bool;

    /// Fetches the contents of `uri` as a `String`.
    fn resolve(&self, uri: &str) -> Result<String>;
}

/// Resolver for reading from stdin when the input is `"-"`.
pub struct StdinResolver;

impl UriResolver for StdinResolver {
    fn can_resolve(&self, uri: &str) -> bool {
        uri == "-"
    }

    fn resolve(&self, _uri: &str) -> Result<String> {
        let mut output = String::new();
        std::io::stdin()
            .read_to_string(&mut output)
            .context(error::StdinReadSnafu)?;
        Ok(output)
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

    fn resolve(&self, uri: &str) -> Result<String> {
        let parsed = Url::parse(uri).context(error::UriSnafu {
            input_source: uri.to_string(),
        })?;
        let path = parsed
            .to_file_path()
            .ok()
            .context(error::FileUriSnafu {
                input_source: uri.to_string(),
            })?;
        fs::read_to_string(path)
            .context(error::FileReadSnafu {
                input_source: uri.to_string(),
            })
    }
}

/// Resolver for `http://` and `https://` URIs.
pub struct HttpResolver;

impl UriResolver for HttpResolver {
    fn can_resolve(&self, uri: &str) -> bool {
        match Url::parse(uri) {
            Ok(parsed) => {
                let s = parsed.scheme();
                s == "http" || s == "https"
            }
            Err(_) => false,
        }
    }

    fn resolve(&self, uri: &str) -> Result<String> {
        // Validate URI
        let parsed = Url::parse(uri).context(error::UriSnafu {
            input_source: uri.to_string(),
        })?;

        // Perform blocking GET
        let resp = blocking::get(parsed)
            .context(error::ReqwestSnafu {
                uri: uri.to_string(),
                method: "GET".to_string(),
            })?
            .error_for_status()
            .context(error::ReqwestSnafu {
                uri: uri.to_string(),
                method: "GET".to_string(),
            })?;

        // Read body
        resp.text()
            .context(error::ReqwestSnafu {
                uri: uri.to_string(),
                method: "GET".to_string(),
            })
    }
}
