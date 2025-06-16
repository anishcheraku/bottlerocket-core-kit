//! This module allows application of settings from URIs or stdin.  The inputs are expected to be
//! TOML settings files, in the same format as user data, or the JSON equivalent.  The inputs are
//! pulled and applied to the API server in a single transaction.

use crate::rando;
use futures::future::{join, ready};
use futures::stream::{self, StreamExt};
use serde::de::{Deserialize, IntoDeserializer};
use snafu::{OptionExt, ResultExt};
use std::{convert::TryFrom, path::Path};
use reqwest::Url;

// bring in our typed URI structs + the trait
use crate::uri_resolver::{StdinUri, FileUri, HttpUri, S3Uri, UriResolver};

// bring in our Snafu context selectors
use crate::apply::error::{
    CommitApplySnafu, InputTypeSnafu, JsonSerializeSnafu, ModelTypeSnafu, MissingSettingsSnafu,
    PatchSnafu, TomlToJsonSnafu, UriSnafu,
};

/// Reads settings in TOML or JSON format from files at the requested URIs (or from stdin, if given
/// "-"), then commits them in a single transaction and applies them to the system.
pub async fn apply<P>(socket_path: P, input_sources: Vec<String>) -> Result<()>
where
    P: AsRef<Path>,
{
    // 1) pair each `&String` with its future
    let mut requests = Vec::with_capacity(input_sources.len());
    for src in &input_sources {
        requests.push(join(ready(src), get(src)));
    }

    // 2) drive up to 4 at once, preserving order
    let responses: Vec<(&String, Result<String>)> =
        stream::iter(requests).buffered(4).collect().await;

    // 3) TOML/JSON → inner JSON
    let mut changes = Vec::with_capacity(responses.len());
    for (src, body_res) in responses {
        let body = body_res?;
        let json = format_change(&body, src)?;
        changes.push((src, json));
    }

    // 4) PATCH each in one tx
    let tx = format!("apiclient-apply-{}", rando());
    for (src, json) in changes {
        let uri = format!("/settings?tx={}", tx);
        let method = "PATCH";
        let (_st, _body) = crate::raw_request(&socket_path, &uri, method, Some(json))
            .await
            .context(PatchSnafu { input_source: src, uri, method })?;
    }

    // 5) commit & apply
    let uri = format!("/tx/commit_and_apply?tx={}", tx);
    let method = "POST";
    let (_st, _body) = crate::raw_request(&socket_path, &uri, method, None)
        .await
        .context(CommitApplySnafu { uri })?;

    Ok(())
}

/// Retrieves the given source location and returns the result in a String.
pub async fn get(input: &str) -> Result<String> {
    // 1) stdin
    if let Ok(resolver) = StdinUri::try_from(input) {
        return resolver.resolve().await;
    }

    // 2) parse once
    let url = Url::parse(input).context(UriSnafu { input_source: input.to_string() })?;

    // 3) file://
    if let Ok(resolver) = FileUri::try_from(url.clone()) {
        return resolver.resolve().await;
    }

    // 4) http(s)://
    if let Ok(resolver) = HttpUri::try_from(url.clone()) {
        return resolver.resolve().await;
    }

    // 5) s3://
    if let Ok(resolver) = S3Uri::try_from(url) {
        return resolver.resolve().await;
    }

    unreachable!("No URI resolver found for `{}`", input);
}

/// Takes a string of TOML or JSON settings data and reserializes
/// it to JSON for sending to the API.
fn format_change(input: &str, input_source: &str) -> Result<String> {
    let mut val = match toml::from_str::<toml::Value>(input) {
        Ok(tv) => {
            let de = tv.into_deserializer();
            serde_json::Value::deserialize(de).context(TomlToJsonSnafu { input_source })?
        }
        Err(toml_err) => {
            serde_json::from_str(input).context(InputTypeSnafu { input_source, toml_err })?
        }
    };

    let obj = val.as_object_mut().context(ModelTypeSnafu { input_source })?;
    let inner = obj.remove("settings").context(MissingSettingsSnafu { input_source })?;
    serde_json::to_string(&inner).context(JsonSerializeSnafu { input_source })
}

pub(crate) mod error {
    use snafu::Snafu;

    #[derive(Debug, Snafu)]
    #[snafu(visibility(pub(crate)))]
    pub enum Error {
        #[snafu(display("Failed to commit combined settings to '{}': {}", uri, source))]
        CommitApply {
            uri: String,
            #[snafu(source(from(crate::Error, Box::new)))]
            source: Box<crate::Error>,
        },

        #[snafu(display("Failed to read given file '{}': {}", input_source, source))]
        FileRead {
            input_source: String,
            source: std::io::Error,
        },

        #[snafu(display("Given invalid file URI '{}'", input_source))]
        FileUri { input_source: String },

        #[snafu(display("Invalid URI '{}': {}", input_source, source))]
        Uri {
            input_source: String,
            source: url::ParseError,
        },

        #[snafu(display("Failed to translate TOML to JSON for API: {}", source))]
        TomlToJson {
            input_source: String,
            source: toml::de::Error,
        },

        #[snafu(display("Input '{}' is not valid JSON: {}", input_source, source))]
        InputType {
            input_source: String,
            toml_err: toml::de::Error,
            #[snafu(source(from(serde_json::Error, Box::new)))]
            source: Box<serde_json::Error>,
        },

        #[snafu(display("Missing top-level 'settings' key in '{}'", input_source))]
        MissingSettings { input_source: String },

        #[snafu(display("Settings from '{}' are not an object", input_source))]
        ModelType { input_source: String },

        #[snafu(display(
            "Failed to {} settings from '{}' to '{}': {}",
            method,
            input_source,
            uri,
            source
        ))]
        Patch {
            input_source: String,
            uri: String,
            method: String,
            #[snafu(source(from(crate::Error, Box::new)))]
            source: Box<crate::Error>,
        },

        #[snafu(display("Failed {} request to '{}': {}", method, uri, source))]
        Reqwest {
            method: String,
            uri: String,
            source: reqwest::Error,
        },

        #[snafu(display("Failed to read from stdin: {}", source))]
        StdinRead { source: std::io::Error },

        #[snafu(display("Failed to serialize settings to JSON: {}", source))]
        JsonSerialize {
            input_source: String,
            source: serde_json::Error,
        },
    }
}

pub use error::Error;
pub type Result<T> = std::result::Result<T, error::Error>;
