use reqwest::{self, Response};
use serde::{Deserialize, Deserializer};
use serde::de::{self, Visitor};

use url::Url;
use anyhow::{Error, anyhow};
use tracing::{info, error};
use std::fmt;


#[derive(Deserialize, Debug, Clone)]
pub(crate) struct EmbyItemData {
    #[serde(rename = "Id", deserialize_with = "deserialize_string_or_int")]
    pub(crate) id: String,
    #[serde(rename = "Name")]
    pub(crate) name: String,
    #[serde(rename = "Path")]
    pub(crate) path: Option<String>,
    #[serde(default, rename = "IndexNumber", deserialize_with = "deserialize_option_string_or_int")]
    pub(crate) episode_num: Option<String>,
    #[serde(default, rename = "ParentIndexNumber", deserialize_with = "deserialize_option_string_or_int")]
    pub(crate) season_num: Option<String>,
}

#[derive(Deserialize, Debug)]
struct EmbySearchResult {
    #[serde(rename = "SearchHints")]
    search_hints: Vec<EmbyItemData>
}

#[derive(Deserialize, Debug)]
struct EmbyItemsResult {
    #[serde(rename = "Items")]
    items: Vec<EmbyItemData>
}

pub(crate) trait EmbySearch {
    async fn search_series(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_seasons_for_series(&self, series_id: &str) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_episodes_for_season(&self, season_id: &str) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_episode_info(&self, episode_id: &str) -> Result<EmbyItemData, Error>;
}

pub(crate) struct EmbyClient {
    emby_url: Url,
    api_key: String,
}

impl EmbyClient {
    pub(crate) async fn new(emby_url: String, api_key: String) -> Result<Self, Error> {
        Ok(EmbyClient {
            emby_url: Url::parse(emby_url.as_str())?,
            api_key
        })
    }

    async fn do_emby_get(&self, url: &str) -> Result<Response, Error> {
        let req_url = self.emby_url.join("/emby/")?.join(url)?;
        info!("doing request against {}", req_url.clone());
        match reqwest::Client::new().get(req_url.clone()).header("X-Emby-Token", self.api_key.as_str()).send().await {
            Ok(r) => {
                Ok(r)
            }
            Err(e) => {
                Err(anyhow!(format!("Error calling {}: {}", req_url.clone(), e)))
            }
        }
    }
}

impl EmbySearch for EmbyClient {
    async fn search_series(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error> {
        let url = format!("Search/Hints?SearchTerm={}&IncludeItemTypes=Series", series_name);
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbySearchResult>(&resp_body) {
                Ok(series) => {
                    Ok(series.search_hints)
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }
    
    async fn get_seasons_for_series(&self, series_id: &str) -> Result<Vec<EmbyItemData>, Error> {
        let url = format!("Shows/{}/Seasons", series_id);
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(series) => {
                    Ok(series.items)
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }
    
    async fn get_episodes_for_season(&self, season_id: &str) -> Result<Vec<EmbyItemData>, Error> {
        let url = format!("Items?ParentId={}&Fields=Path&IsMissing=false&SortBy=PremiereDate", season_id);
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(series) => {
                    Ok(series.items)
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }

    async fn get_episode_info(&self, episode_id: &str) -> Result<EmbyItemData, Error> {
        let url = format!("Items?Ids={}&Fields=Path&IsMissing=false&SortBy=PremiereDate", episode_id);
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(episodes) => {
                    match episodes.items.get(0) {
                        Some(episode) => {
                            Ok(episode.clone())
                        }
                        None => {
                            let err_msg = format!("Somehow could not find item id {}", episode_id);
                            error!(err_msg);
                            Err(anyhow!(err_msg))
                        }
                    }
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }
}

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrIntVisitor;

    impl<'de> Visitor<'de> for StringOrIntVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or an int")
        }

        fn visit_i64<E>(self, value: i64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_u64<E>(self, value: u64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_str<E>(self, value: &str) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_owned())
        }
    }

    deserializer.deserialize_any(StringOrIntVisitor)
}

fn deserialize_option_string_or_int<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionStringOrIntVisitor;

    impl<'de> Visitor<'de> for OptionStringOrIntVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or an int")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_str<E>(self, value: &str) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_owned()))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

    }

    deserializer.deserialize_any(OptionStringOrIntVisitor)
}