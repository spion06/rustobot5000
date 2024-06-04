
use reqwest::{self, Response};
use serde::{Deserialize, Deserializer};
use serde::de::{self, Visitor};

use strum::{Display, EnumIter, EnumString};
use url::Url;
use anyhow::{Error, anyhow};
use tracing::{info, error};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc};
use tokio::sync::Mutex as TokioMutex;



#[derive(Deserialize, Debug, Clone)]
pub(crate) struct EmbyItemData {
    #[serde(rename = "Id", deserialize_with = "deserialize_string_or_int")]
    pub(crate) id: String,
    #[serde(rename = "Name")]
    pub(crate) name: String,
    #[serde(rename = "Type")]
    pub(crate) item_type: String,
    #[serde(rename = "Path")]
    pub(crate) path: Option<String>,
    #[serde(default, rename = "IndexNumber", deserialize_with = "deserialize_option_string_or_int")]
    pub(crate) episode_num: Option<String>,
    #[serde(default, rename = "ParentIndexNumber", deserialize_with = "deserialize_option_string_or_int")]
    pub(crate) season_num: Option<String>,
    #[serde(default, rename = "UserData")]
    pub(crate) user_data: Option<EmbyItemUserData>,
}

#[derive(Debug, EnumString, Display, Default, EnumIter)]
pub(crate) enum SearchItemType {
    #[default]
    #[strum(ascii_case_insensitive)]
    Series,
    #[strum(ascii_case_insensitive)]
    Movie,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct EmbyItemUserData {
    #[serde(rename = "Played")]
    pub(crate) played: bool
}

#[derive(Deserialize, Debug)]
struct EmbySearchResult {
    #[serde(default, rename = "SearchHints")]
    search_hints: Vec<EmbyItemData>
}

#[derive(Deserialize, Debug)]
struct EmbyItemsResult {
    #[serde(default, rename = "Items")]
    items: Vec<EmbyItemData>
}

impl EmbyItemsResult {
    pub fn get_sorted_items(&self) -> Vec<EmbyItemData> {
        let mut items = self.items.clone();
        items.sort_by(|a, b| {
            let a_int : u32 = a.episode_num.clone().unwrap_or("0".to_string()).parse().unwrap_or(0);
            let b_int : u32 = b.episode_num.clone().unwrap_or("0".to_string()).parse().unwrap_or(0);
            a_int.cmp(&b_int)
        });
        items
    }
}

pub(crate) trait EmbySearch {
    async fn search_items(&self, item_name: &str, item_type: Vec<SearchItemType>) -> Result<Vec<EmbyItemData>, Error>;
    async fn search_series(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error>;
    async fn search_movies(&self, movie_name: &str) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_seasons_for_series(&self, series_id: &str) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_episodes_for_season(&self, season_id: &str, user: &Option<EmbyItemData>) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_item_info(&self, episode_id: &str) -> Result<EmbyItemData, Error>;
    async fn get_all_series(&self) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_all_movies(&self) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_users(&self) -> Result<Vec<EmbyItemData>, Error>;
    async fn get_user_by_id(&self, user_id: String) -> Result<EmbyItemData, Error>;
    async fn user_stop_fn(&self, user_id: String, media_id: String) -> Arc<TokioMutex<Pin<Box<dyn Future<Output = bool> + Send>>>>;
}

#[derive(Clone)]
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

    async fn do_emby_post(&self, url: &str) -> Result<Response, Error> {
        let req_url = self.emby_url.join("/emby/")?.join(url)?;
        info!("doing post request against {}", req_url.clone());
        match reqwest::Client::new().post(req_url.clone()).header("X-Emby-Token", self.api_key.as_str()).send().await {
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
    async fn search_items(&self, item_name: &str, item_types: Vec<SearchItemType>) -> Result<Vec<EmbyItemData>, Error> {
        if item_name.len() == 0 {
            return Err(anyhow!("no item types for search passed!"))
        }
        let item_types = item_types.iter().map(|i| i.to_string()).collect::<Vec<String>>().join(",");
        let url = format!("Items?Recursive=true&IncludeItemTypes={}&SortBy=SortName&SearchTerm={}", item_types, item_name);
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

    async fn search_series(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error> {
        self.search_items(series_name, vec![SearchItemType::Series]).await
    }

    async fn search_movies(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error> {
        self.search_items(series_name, vec![SearchItemType::Movie]).await
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
    
    async fn get_episodes_for_season(&self, season_id: &str, user: &Option<EmbyItemData>) -> Result<Vec<EmbyItemData>, Error> {
        let url_prefix = match user {
            Some(u) => format!("Users/{}/", u.id),
            None => "".to_string(),
        };
        let url = format!("{}Items?ParentId={}&Fields=Path&IsMissing=false&SortBy=PremiereDate", url_prefix, season_id);
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(series) => {
                    Ok(series.get_sorted_items())
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }

    async fn get_item_info(&self, item_id: &str) -> Result<EmbyItemData, Error> {
        let url = format!("Items?Ids={}&Fields=Path&IsMissing=false&SortBy=PremiereDate", item_id);
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
                            let err_msg = format!("Somehow could not find item id {}", item_id);
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

    async fn get_all_series(&self) -> Result<Vec<EmbyItemData>, Error> {
        let url = "Items?Recursive=true&IncludeItemTypes=Series&SortBy=SortName";
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

    async fn get_all_movies(&self) -> Result<Vec<EmbyItemData>, Error> {
        let url = "Items?Recursive=true&IncludeItemTypes=Movie&SortBy=SortName";
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

    async fn get_users(&self) -> Result<Vec<EmbyItemData>, Error> {
        let url = "Users/Query";
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(series) => {
                    Ok(series.items)
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing user data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting user data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }

    async fn get_user_by_id(&self, user_id: String) -> Result<EmbyItemData, Error> {
        let url = format!("Users/{user_id}");
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemData>(&resp_body) {
                Ok(user) => {
                    Ok(user)
                }
                Err(e) => {
                    Err(anyhow!(format!("error deserializing user data {}: {}", e, String::from_utf8_lossy(&resp_body))).into())
                }
            }
        } else {
            Err(anyhow!(format!("error getting user data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body))).into())
        }
    }

    async fn user_stop_fn(&self, user_id: String, media_id: String) -> Arc<TokioMutex<Pin<Box<dyn Future<Output = bool> + Send>>>> {
        let emby_client = self.clone();
        Arc::new(TokioMutex::new(Box::pin(async move {
                let url = format!("Users/{user_id}/PlayedItems/{media_id}");
                let _resp = emby_client.do_emby_post(&url).await;
                true
        }) as Pin<Box<dyn Future<Output = bool> + Send>>))
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