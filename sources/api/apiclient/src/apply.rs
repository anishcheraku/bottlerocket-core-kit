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
    // We want to retrieve URIs in parallel because they're arbitrary and could be slow.  First, we
    // build a list of request futures, and we store the source of the data with the future for
    // inclusion in later error messages.
    let mut get_requests = Vec::with_capacity(input_sources.len());
    for input_source in &input_sources {
        let get_future = get(input_source);
        let info_future = ready(input_source);
        get_requests.push(join(info_future, get_future));
    }

    // Stream out the requests and await responses (in order).
    let get_request_stream = stream::iter(get_requests).buffered(4);
    let get_responses: Vec<(&String, Result<String>)> = get_request_stream.collect().await;

    // Reformat the responses to JSON we can send to the API.
    let mut changes = Vec::with_capacity(get_responses.len());
    for (input_source, get_response) in get_responses {
        let response = get_response?;
        let json = format_change(&response, input_source)?;
        changes.push((input_source, json));
    }

    // We use a specific transaction ID so we don't commit any other changes that may be pending.
    let transaction = format!("apiclient-apply-{}", rando());

    // Send the settings changes to the server in the same transaction.  (They're quick local
    // requests, so don't add the complexity of making them run concurrently.)
    for (input_source, json) in changes {
        let uri = format!("/settings?tx={}", transaction);
        let method = "PATCH";
        let (_status, _body) = crate::raw_request(&socket_path, &uri, method, Some(json))
            .await
            .context(error::PatchSnafu {
                input_source,
                uri,
                method,
            })?;
    }

    // Commit the transaction and apply it to the system.
    let uri = format!("/tx/commit_and_apply?tx={}", transaction);
    let method = "POST";
    let (_status, _body) = crate::raw_request(&socket_path, &uri, method, None)
        .await
        .context(error::CommitApplySnafu { uri })?;

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
    // Try to parse the input as (arbitrary) TOML.  If that fails, try to parse it as JSON.
    let mut json_val = match toml::from_str::<toml::Value>(input) {
        Ok(toml_val) => {
            // We need JSON for the API.  serde lets us convert between Deserialize-able types by
            // reusing the deserializer.  Turn the TOML value into a JSON value.
            let d = toml_val.into_deserializer();
            serde_json::Value::deserialize(d).context(error::TomlToJsonSnafu { input_source })?
        }
        Err(toml_err) => {
            // TOML failed, try JSON; include the toml parsing error, because if they intended to
            // give TOML we should still tell them what was wrong with it.
            serde_json::from_str(input).context(error::InputTypeSnafu {
                input_source,
                toml_err,
            })
        }?,
    };

    // Remove outer "settings" layer before sending to API or deserializing it into the model,
    // neither of which expects it.
    let json_object = json_val
        .as_object_mut()
        .context(error::ModelTypeSnafu { input_source })?;
    let json_inner = json_object
        .remove("settings")
        .context(error::MissingSettingsSnafu { input_source })?;
    // Return JSON text we can send to the API.
    serde_json::to_string(&json_inner).context(error::JsonSerializeSnafu { input_source })
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
