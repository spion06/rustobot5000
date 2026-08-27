
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
    pub(crate) item_type: Option<String>,
    #[serde(rename = "Path")]
    pub(crate) path: Option<String>,
    #[serde(default, rename = "IndexNumber", deserialize_with = "deserialize_option_string_or_int")]
    pub(crate) episode_num: Option<String>,
    #[serde(default, rename = "ParentIndexNumber", deserialize_with = "deserialize_option_string_or_int")]
    pub(crate) season_num: Option<String>,
    #[serde(default, rename = "UserData")]
    pub(crate) user_data: Option<EmbyItemUserData>,
}

#[derive(Debug, Clone, EnumString, Display, Default, EnumIter)]
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
#[allow(dead_code)]
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
            let a_int: u32 = a.episode_num.as_deref().unwrap_or("0").parse().unwrap_or(0);
            let b_int: u32 = b.episode_num.as_deref().unwrap_or("0").parse().unwrap_or(0);
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

    /// Helper function to fetch and deserialize Emby API responses
    async fn fetch_emby_items(&self, url: &str) -> Result<Vec<EmbyItemData>, Error> {
        let resp = self.do_emby_get(url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(result) => Ok(result.items),
                Err(e) => Err(anyhow!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body)))
            }
        } else {
            Err(anyhow!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body)))
        }
    }

    /// Helper function to fetch sorted items (for episodes)
    async fn fetch_emby_items_sorted(&self, url: &str) -> Result<Vec<EmbyItemData>, Error> {
        let resp = self.do_emby_get(url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(result) => Ok(result.get_sorted_items()),
                Err(e) => Err(anyhow!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body)))
            }
        } else {
            Err(anyhow!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body)))
        }
    }
}

impl EmbySearch for EmbyClient {
    async fn search_items(&self, item_name: &str, item_types: Vec<SearchItemType>) -> Result<Vec<EmbyItemData>, Error> {
        if item_name.is_empty() {
            return Err(anyhow!("no search term provided"))
        }
        if item_types.is_empty() {
            return Err(anyhow!("no item types for search passed"))
        }
        let item_types_str = item_types.iter().map(|i| i.to_string()).collect::<Vec<String>>().join(",");
        let url = format!("Items?Recursive=true&IncludeItemTypes={}&SortBy=SortName&SearchTerm={}", item_types_str, item_name);
        self.fetch_emby_items(&url).await
    }

    async fn search_series(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error> {
        self.search_items(series_name, vec![SearchItemType::Series]).await
    }

    async fn search_movies(&self, series_name: &str) -> Result<Vec<EmbyItemData>, Error> {
        self.search_items(series_name, vec![SearchItemType::Movie]).await
    }

    async fn get_seasons_for_series(&self, series_id: &str) -> Result<Vec<EmbyItemData>, Error> {
        let url = format!("Shows/{}/Seasons", series_id);
        self.fetch_emby_items(&url).await
    }
    
    async fn get_episodes_for_season(&self, season_id: &str, user: &Option<EmbyItemData>) -> Result<Vec<EmbyItemData>, Error> {
        let url_prefix = match user {
            Some(u) => format!("Users/{}/", u.id),
            None => "".to_string(),
        };
        let url = format!("{}Items?ParentId={}&Fields=Path&IsMissing=false&SortBy=PremiereDate", url_prefix, season_id);
        self.fetch_emby_items_sorted(&url).await
    }

    async fn get_item_info(&self, item_id: &str) -> Result<EmbyItemData, Error> {
        let url = format!("Items?Ids={}&Fields=Path&IsMissing=false&SortBy=PremiereDate", item_id);
        let resp = self.do_emby_get(&url).await?;
        let resp_status = resp.status();
        let resp_body = resp.bytes().await?;
        if resp_status.clone().is_success() {
            match serde_json::from_slice::<EmbyItemsResult>(&resp_body) {
                Ok(episodes) => {
                    match episodes.items.first() {
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
                    Err(anyhow!("error deserializing data {}: {}", e, String::from_utf8_lossy(&resp_body)))
                }
            }
        } else {
            Err(anyhow!("error getting data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body)))
        }
    }

    async fn get_all_series(&self) -> Result<Vec<EmbyItemData>, Error> {
        self.fetch_emby_items("Items?Recursive=true&IncludeItemTypes=Series&SortBy=SortName").await
    }

    async fn get_all_movies(&self) -> Result<Vec<EmbyItemData>, Error> {
        self.fetch_emby_items("Items?Recursive=true&IncludeItemTypes=Movie&SortBy=SortName").await
    }

    async fn get_users(&self) -> Result<Vec<EmbyItemData>, Error> {
        self.fetch_emby_items("Users/Query").await
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
                    Err(anyhow!("error deserializing user data {}: {}", e, String::from_utf8_lossy(&resp_body)))
                }
            }
        } else {
            Err(anyhow!("error getting user data {}: {}", resp_status.as_str(), String::from_utf8_lossy(&resp_body)))
        }
    }

    async fn user_stop_fn(&self, user_id: String, media_id: String) -> Arc<TokioMutex<Pin<Box<dyn Future<Output = bool> + Send>>>> {
        let emby_client = self.clone();
        Arc::new(TokioMutex::new(Box::pin(async move {
                let url = format!("Users/{user_id}/PlayedItems/{media_id}");
                match emby_client.do_emby_post(&url).await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            info!("Successfully marked item {} as played for user {}", media_id, user_id);
                            true
                        } else {
                            error!("Failed to mark item {} as played: status {}", media_id, resp.status());
                            false
                        }
                    }
                    Err(e) => {
                        error!("Error marking item {} as played: {}", media_id, e);
                        false
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_emby_item_with_string_id() {
        let json = r#"{
            "Id": "abc123",
            "Name": "Test Show",
            "Type": "Series"
        }"#;
        let item: EmbyItemData = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, "abc123");
        assert_eq!(item.name, "Test Show");
        assert_eq!(item.item_type, Some("Series".to_string()));
    }

    #[test]
    fn test_deserialize_emby_item_with_int_id() {
        let json = r#"{
            "Id": 12345,
            "Name": "Test Movie",
            "Type": "Movie"
        }"#;
        let item: EmbyItemData = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, "12345");
        assert_eq!(item.name, "Test Movie");
        assert_eq!(item.item_type, Some("Movie".to_string()));
    }

    #[test]
    fn test_deserialize_emby_item_with_episode_numbers() {
        let json = r#"{
            "Id": "ep1",
            "Name": "Pilot",
            "Type": "Episode",
            "IndexNumber": 1,
            "ParentIndexNumber": 1,
            "Path": "/media/shows/pilot.mkv"
        }"#;
        let item: EmbyItemData = serde_json::from_str(json).unwrap();
        assert_eq!(item.episode_num, Some("1".to_string()));
        assert_eq!(item.season_num, Some("1".to_string()));
        assert_eq!(item.path, Some("/media/shows/pilot.mkv".to_string()));
    }

    #[test]
    fn test_deserialize_emby_item_with_string_episode_numbers() {
        let json = r#"{
            "Id": "ep1",
            "Name": "Pilot",
            "IndexNumber": "5",
            "ParentIndexNumber": "2"
        }"#;
        let item: EmbyItemData = serde_json::from_str(json).unwrap();
        assert_eq!(item.episode_num, Some("5".to_string()));
        assert_eq!(item.season_num, Some("2".to_string()));
    }

    #[test]
    fn test_deserialize_emby_item_with_user_data() {
        let json = r#"{
            "Id": "123",
            "Name": "Watched Episode",
            "UserData": {
                "Played": true
            }
        }"#;
        let item: EmbyItemData = serde_json::from_str(json).unwrap();
        assert!(item.user_data.is_some());
        assert!(item.user_data.unwrap().played);
    }

    #[test]
    fn test_deserialize_emby_item_minimal() {
        let json = r#"{
            "Id": "min1",
            "Name": "Minimal Item"
        }"#;
        let item: EmbyItemData = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, "min1");
        assert_eq!(item.name, "Minimal Item");
        assert!(item.item_type.is_none());
        assert!(item.path.is_none());
        assert!(item.episode_num.is_none());
        assert!(item.season_num.is_none());
        assert!(item.user_data.is_none());
    }

    #[test]
    fn test_emby_items_result_sorting() {
        let json = r#"{
            "Items": [
                {"Id": "3", "Name": "Episode 10", "IndexNumber": 10},
                {"Id": "1", "Name": "Episode 1", "IndexNumber": 1},
                {"Id": "2", "Name": "Episode 5", "IndexNumber": 5}
            ]
        }"#;
        let result: EmbyItemsResult = serde_json::from_str(json).unwrap();
        let sorted = result.get_sorted_items();
        assert_eq!(sorted[0].name, "Episode 1");
        assert_eq!(sorted[1].name, "Episode 5");
        assert_eq!(sorted[2].name, "Episode 10");
    }

    #[test]
    fn test_search_item_type_from_str() {
        use std::str::FromStr;
        assert!(SearchItemType::from_str("Series").is_ok());
        assert!(SearchItemType::from_str("series").is_ok());
        assert!(SearchItemType::from_str("SERIES").is_ok());
        assert!(SearchItemType::from_str("Movie").is_ok());
        assert!(SearchItemType::from_str("movie").is_ok());
        assert!(SearchItemType::from_str("Invalid").is_err());
    }
}
