use embyclient::{EmbyClient, EmbyItemData, EmbySearch};
use poise::{samples::HelpConfiguration, serenity_prelude::{self as serenity, ComponentInteractionDataKind, CreateActionRow, CreateAttachment, CreateSelectMenuKind, CreateSelectMenuOption}, CreateReply};
use kube::{ api::{ListParams, LogParams}, Api, Client as KubeClient};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Pod};
use uuid::Uuid;
use std::{fmt, str::FromStr, sync::Arc};
use tracing::{info, error, warn};
use tracing_subscriber;
use tokio::{signal::unix::{signal, SignalKind}, sync::{Mutex, MutexGuard}};
mod gstreamer;
mod embyclient;
use gstreamer::PlayQueue;
extern crate gstreamer as gst;

#[derive(Debug, poise::Modal)]
#[allow(dead_code)]
struct ShowSearch {
    show_name: String,
    show_season: Option<String>,
}

// Define a custom error type
#[derive(Debug)]
struct BotError {
    details: String,
}

struct EmbySearchResult {
    result_box: CreateSelectMenuKind,
    result_items: usize,
}

impl BotError {
    fn new(msg: &str) -> BotError {
        BotError{details: msg.to_string()}
    }
}

impl fmt::Display for BotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

fn bot_error(msg: &str) -> Error {
    return Box::new(BotError::new(msg))
}

impl std::error::Error for BotError {}

struct Data {
    video_pipeline: Arc<Mutex<PlayQueue>>,
    emby_client: Arc<EmbyClient>,
} // User data, which is stored and accessible in all command invocations
impl Data {
    pub async fn load(_ctx: &serenity::Context, video_pipeline: Arc<Mutex<PlayQueue>>, emby_client: EmbyClient) -> Self {
        Self {
            video_pipeline: video_pipeline,
            emby_client: Arc::new(emby_client),
        }
    }

    fn clone(&self) -> Data {
        Data {
            video_pipeline: Arc::clone(&self.video_pipeline),
            emby_client: Arc::clone(&self.emby_client),
        }
    }
    async fn get_kube_client(&self) -> Result<KubeClient, Error> {
        match KubeClient::try_default().await {
            Ok(client) => {
                Ok(client)
            }
            Err(e) => {
                error!("error getting kube client {}", e);
                Err(Box::new(e))
            }
        }
    }
    async fn get_deployment_client(&self) -> Result<Api<Deployment>, Error> {
        let kube_client = self.get_kube_client().await?;
        let api_client: Api<Deployment> = Api::default_namespaced(kube_client);
        Ok(api_client)
    }

    async fn get_pipeline_ref(&self) -> MutexGuard<'_, PlayQueue> {
        self.video_pipeline.lock().await
    }
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// validate the game. throws an error if the name is not valid or there is some other issue
async fn validate_game_name(ctx: Context<'_>, game: String) -> Result<(), Error> {
    match ctx.data().get_deployment_client().await {
        Ok(client) => {
            if get_valid_deployments(client).await.unwrap().contains(&game) {
                info!("{game} is a valid game name");
                return Ok(())
            } else {
                info!("{game} is not a valid game name");
                return Err(Box::new(BotError::new(&format!("{game} is not a valid game name"))))
            }
        },
        Err(e) => Err(e)
    }
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn queue_video(
    ctx: Context<'_>,
    #[description = "path to a video to play"] url: String,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.add_uri(url.clone(), url.clone().split("/").last().unwrap().to_string()) {
        Ok(_) => {
            ctx.say("queued video").await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("error setting the source uri: {}", e);
            ctx.say(err_msg.clone()).await?;
            error!(err_msg);
            Err(bot_error(err_msg.as_str()))
        }
    }
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn play_video(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.start_playback() {
        Ok(_) => {
            ctx.say("played video").await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("error starting playback: {}", e);
            ctx.say(err_msg.clone()).await?;
            error!(err_msg);
            Err(bot_error(err_msg.as_str()))
        }
    }
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn stop_video(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.stop_playback() {
        Ok(_) => {
            ctx.say("stopped video").await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("error setting stopping video: {}", e);
            ctx.say(err_msg.clone()).await?;
            error!(err_msg);
            Err(bot_error(err_msg.as_str()))
        }
    }
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn pause_video(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.pause_playback() {
        Ok(_) => {
            ctx.say("paused current video").await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("error pausing video: {}", e);
            ctx.say(err_msg.clone()).await?;
            error!(err_msg);
            Err(bot_error(err_msg.as_str()))
        }
    }
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn skip_video(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.skip_video() {
        Ok(_) => {
            ctx.say("skipped video").await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("error skipping video: {}", e);
            ctx.say(err_msg.clone()).await?;
            error!(err_msg);
            Err(bot_error(err_msg.as_str()))
        }
    }
}

/// list all the available games to restart
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn game_list(
    ctx: Context<'_>
) -> Result<(), Error> {
    match ctx.data().get_deployment_client().await {
        Ok(client) => {
            let deps = get_valid_deployments(client).await?;
            let response = String::from("Valid Deployment targets:\n") + &deps.join("\n");
            ctx.say(response).await?;
            Ok(())
        },
        Err(e) => {
            error!("got an error listing deployments: {}", e);
            Err(e)
        }
    }
}

/// restart a game
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn game_restart(
    ctx: Context<'_>,
    #[description = "Game to restart"] game: String,
) -> Result<(), Error> {
    validate_game_name(ctx, game.clone()).await?;
    match ctx.data().get_deployment_client().await {
        Ok(client) => {
            restart_deployment(client.clone(), game.clone()).await?;
            ctx.say(format!("Started restart on {game}")).await?;
            ctx.say("Check status with game_status command").await?;
            return Ok(())
        },
        Err(e) => {
            let err_msg = format!("Error getting client: {e}");
            error!("{err_msg}");
            return Err(e)
        }
    }
}

async fn get_deployment_pods(
    client: KubeClient,
    deployment_name: String
) -> Result<Vec<Pod>, Error> {
    let dep_client: Api<Deployment> = Api::default_namespaced(client.clone());
    let pod_client: Api<Pod> = Api::default_namespaced(client);
    let deployment = dep_client.get(&deployment_name).await?;
    let pod_match_labels = deployment.spec.unwrap().selector.match_labels.unwrap();
    let selector_query = pod_match_labels.iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>()
        .join(",");
    let lp = ListParams::default().labels(&selector_query);
    let pods = pod_client.list(&lp).await?;
    Ok(pods.items)
}

/// get the current status of a game. should be in running for "normal" operation
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn game_status(
    ctx: Context<'_>,
    #[description = "Game to restart"] game: String,
) -> Result<(), Error> {
    validate_game_name(ctx, game.clone()).await?;
    match ctx.data().get_kube_client().await {
        Ok(kclient) => {
            let d_client: Api<Deployment> = Api::default_namespaced(kclient.clone());
            let resp = d_client.get_status(&game).await?;
            let status = resp.status.expect("somehow there is no deployment status");
            let total_replicas = status.replicas.unwrap_or_else(|| {
                warn!("total_replicas not found found for {game}");
                0
            });
            let ready_replicas = status.ready_replicas.unwrap_or_else(|| {
                warn!("ready_replicas not found for {game}");
                0
            });
            let pods = get_deployment_pods(kclient, game.clone()).await?;
            ctx.say(format!("{ready_replicas}/{total_replicas} ready for game {game}")).await?;
            for pod in pods {
                let pod_status = pod.status.expect("pod has no status somehow").phase.unwrap_or("unknown".to_string());
                ctx.say(format!("Pod in status: {pod_status} ")).await?;
            }
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("Error getting client: {e}");
            error!("{err_msg}");
            Err(e)
        }
    }
}


/// get logs from a game
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn game_logs(
    ctx: Context<'_>,
    #[description = "Game to get the logs for"] game: String,
    #[description = "How many log lines to get"] lines: Option<i64>
) -> Result<(), Error> {
    validate_game_name(ctx, game.clone()).await?;
    match ctx.data().get_kube_client().await {
        Ok(kclient) => {
            let pods = get_deployment_pods(kclient.clone(), game.clone()).await?;
            let pod_client: Api<Pod> = Api::default_namespaced(kclient.clone());
            let tail_lines = lines.unwrap_or(10).min(100);
            for pod in pods {
                let log_params = LogParams {
                    tail_lines: Some(tail_lines),
                    ..LogParams::default()
                };
                info!("getting last {tail_lines} lines from {game}");
                let pod_logs = pod_client.logs(&pod.metadata.name.unwrap(), &log_params).await?;
                let attachment_name = format!("{game}.log");
                let attachment_logs = CreateAttachment::bytes(pod_logs.as_bytes(), attachment_name);
                ctx.send(CreateReply::default().attachment(attachment_logs)).await?;
            }
            Ok(())
        }
        Err(e) => {
            error!("Error getting client {e}");
            Err(e)
        }
    }
}

/// Show help message
#[poise::command(prefix_command, track_edits, category = "Utility")]
async fn help(
    ctx: Context<'_>,
    #[description = "Command to get help for"]
    #[rest]
    mut command: Option<String>,
) -> Result<(), Error> {
    // This makes it possible to just make `help` a subcommand of any command
    // `/fruit help` turns into `/help fruit`
    // `/fruit help apple` turns into `/help fruit apple`
    if ctx.invoked_command_name() != "help" {
        command = match command {
            Some(c) => Some(format!("{} {}", ctx.invoked_command_name(), c)),
            None => Some(ctx.invoked_command_name().to_string()),
        };
    }
    let extra_text_at_bottom = "\
Type `?help command` for more info on a command.
You can edit your `?help` message to the bot and the bot will edit its response.";

    let config = HelpConfiguration {
        show_subcommands: true,
        show_context_menu_commands: true,
        ephemeral: true,
        extra_text_at_bottom,

        ..Default::default()
    };
    poise::builtins::help(ctx, command.as_deref(), config).await?;
    Ok(())
}

#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn shutdown(ctx: Context<'_>) -> Result<(), Error> {
    ctx.framework().shard_manager().shutdown_all().await;
    Ok(())
}

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn rusto_player(ctx: Context<'_>) -> Result<(), Error> {
    let uuid_boop = ctx.id();
    let buttons = vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("{uuid_boop}_play"))
                .style(serenity::ButtonStyle::Primary)
                .label("play")
                .emoji('\u{25B6}'),
            serenity::CreateButton::new(format!("{uuid_boop}_pause"))
                .style(serenity::ButtonStyle::Primary)
                .label("pause")
                .emoji('\u{23F8}'),
            serenity::CreateButton::new(format!("{uuid_boop}_stop"))
                .style(serenity::ButtonStyle::Primary)
                .label("stop")
                .emoji('\u{23F9}'),
            serenity::CreateButton::new(format!("{uuid_boop}_skip"))
                .style(serenity::ButtonStyle::Primary)
                .label("skip")
                .emoji('\u{23ED}'),
        ]),
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("{uuid_boop}_search"))
                .style(serenity::ButtonStyle::Primary)
                .label("search")
                .emoji('\u{1F50D}'),
            serenity::CreateButton::new(format!("{uuid_boop}_show_queue"))
                .style(serenity::ButtonStyle::Primary)
                .label("queue")
                .emoji('\u{1F4DC}'),
            serenity::CreateButton::new(format!("{uuid_boop}_now_playing"))
                .style(serenity::ButtonStyle::Primary)
                .label("now playing")
                .emoji('\u{1F3A6}'),
        ]),
    ];

    let reply = {
        CreateReply::default()
            .content("I want to watch something \u{1F346}")
            .components(buttons.clone())
    };

    ctx.send(reply).await?;

    while let Some(mci) = serenity::ComponentInteractionCollector::new(ctx)
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .timeout(std::time::Duration::from_secs(3600))
        .filter(move |mci| mci.data.custom_id.starts_with(&uuid_boop.to_string()))
        .await
    {
        let mut send_final = true;
        let mut msg = mci.message.clone();
        let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
        if mci.data.custom_id.ends_with("play") {
            match &pipeline_ref.start_playback() {
                Ok(_v) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(get_now_playing(&pipeline_ref).await)
                    ).await?;
                },
                Err(e) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Error starting playback {}", e))
                    ).await?;
                }
            }
        }
        if mci.data.custom_id.ends_with("now_playing") {
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(get_now_playing(&pipeline_ref).await)
            ).await?;
        }
        if mci.data.custom_id.ends_with("pause") {
            match &pipeline_ref.pause_playback() {
                Ok(_) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Video Paused"))
                    ).await?;
                },
                Err(e) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Error Pausing {}", e))
                    ).await?;
                }
            }
        }
        if mci.data.custom_id.ends_with("stop") {
            match &pipeline_ref.stop_playback() {
                Ok(_) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Video Stopped"))
                    ).await?;
                },
                Err(e) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Error Stopping {}", e))
                    ).await?;
                }
            }
        }
        if mci.data.custom_id.ends_with("skip") {
            match &pipeline_ref.skip_video() {
                Ok(_) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Video Skipped"))
                    ).await?;
                },
                Err(e) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Error Skipped {}", e))
                    ).await?;
                }
            }
        }
        if mci.data.custom_id.ends_with("show_queue") {
            let result_box = get_queue_selector(&pipeline_ref, uuid_boop.to_string().as_str()).await;
            msg.edit(
                ctx,
                serenity::EditMessage::new().components(buttons.clone().iter().chain(result_box.iter()).cloned().collect())
            ).await?;
        }

        // handle click on queue item to remove
        if mci.data.custom_id.ends_with("queue_list") {
            let queue_item = match &mci.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => &values[0],
                k => {
                    warn!("got an unknown selection kind on show_queue {:#?}", k);
                    "unknown"
                }
            };
            if queue_item == "empty" || queue_item == "unknown" {
                info!("queue item is {}", queue_item)
            } else {
                msg.edit(
                    ctx,
                    serenity::EditMessage::new().content(format!("Removing item {}", queue_item))
                ).await?;
                pipeline_ref.remove_uri(&Uuid::from_str(queue_item).unwrap())?;
                let result_box = get_queue_selector(&pipeline_ref, uuid_boop.to_string().as_str()).await;
                msg.edit(
                    ctx,
                    serenity::EditMessage::new().components(buttons.clone().iter().chain(result_box.iter()).cloned().collect())
                ).await?;
            }
        }

        // handle result from clicking on a series
        if mci.data.custom_id.ends_with("series_result") {
            let series_id = match &mci.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => &values[0],
                k => {
                    warn!("got an unknown selection kind on series {:#?}", k);
                    "unknown"
                }
            };
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(format!("Got series {}", series_id))
            ).await?;
            let mut result_box: Vec<CreateActionRow> = vec![];
            let mut message: String = "No results found".to_string();
            match get_seasons(ctx.data().emby_client.as_ref(), series_id).await {
                Ok(seasons) => {
                    result_box.push(
                        serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_season_result", uuid_boop), seasons.result_box).placeholder(format!("{} Seasons", seasons.result_items))),
                    );
                    message = format!("Found {} Seasons", seasons.result_items);
                }
                Err(e) => {
                    message = format!("Error getting seasons: {}", e);
                }
            }
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message).components(buttons.clone().iter().chain(result_box.iter()).cloned().collect())
            ).await?;
        }

        // handle result from clicking on a season
        if mci.data.custom_id.ends_with("season_result") {
            let season_id = match &mci.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => &values[0],
                _ => {
                    warn!("got an unknown selection kind on seasons");
                    "unknown"
                }
            };
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(format!("Got Season {}", season_id))
            ).await?;
            let mut result_box: Vec<CreateActionRow> = vec![];
            let mut message: String = "No results found".to_string();
            match get_episodes(ctx.data().emby_client.as_ref(), season_id).await {
                Ok(seasons) => {
                    result_box.push(
                        serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_episodes_result", uuid_boop), seasons.result_box).placeholder(format!("{} Series Episodes", seasons.result_items))),
                    );
                    message = format!("Found {} Episodes", seasons.result_items);
                }
                Err(e) => {
                    message = format!("Error getting episodes: {}", e);
                }
            }
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message).components(buttons.clone().iter().chain(result_box.iter()).cloned().collect())
            ).await?;
        }

        // handle result from clicking on an episode (IE queue the item)
        if mci.data.custom_id.ends_with("episodes_result") {
            let episode_id = match &mci.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => &values[0],
                _ => {
                    warn!("got an unknown selection kind on episodes");
                    "unknown"
                }
            };
            let mut message: String = "No results found".to_string();
            let episode_info = ctx.data().emby_client.as_ref().get_episode_info(episode_id).await?;
            let episode_path = match episode_info.clone().path {
                Some(path) => path,
                None => "".to_string(),
            };
            if episode_path.is_empty() {
                message = format!("could not find episode info for {}", episode_info.name);
                error!(message)
            } else {
                msg.edit(
                    ctx,
                    serenity::EditMessage::new().content(format!("Got episode {}", episode_path))
                ).await?;
                let episode_path = episode_path.replace("/mnt/storage", "/mnt/zfspool/storage");
                match &pipeline_ref.add_uri(episode_path.to_string(), generate_episode_name(episode_info.clone())) {
                    Ok(i) => {
                        message = format!("added {} to queue", i.name());
                    }
                    Err(e) => {
                        message = format!("error adding {} to queue: {}", episode_path, e);
                        error!(message)
                    }
                    
                };
            };
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message)
            ).await?;
        }
        if mci.data.custom_id.ends_with("search") {
            let data = poise::execute_modal_on_component_interaction::<ShowSearch>(ctx, mci.clone(), None, None).await;
            let mut result_box: Vec<CreateActionRow> = vec![];
            let mut message: String = "No results found".to_string();
            match &data {
                Ok(d) => {
                    send_final = false;
                    match d {
                        Some(user_search) => {
                            match get_series(ctx.data().emby_client.as_ref(), &user_search.show_name).await {
                                Ok(list) => {
                                    if list.result_items == 0 {
                                        let empty_result = CreateSelectMenuKind::String { options: vec![CreateSelectMenuOption::new("No Results found!", "empty")] };
                                        result_box.push(
                                            serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_series_result", uuid_boop), empty_result).placeholder("Series Search Results")),
                                        )
                                    } else {
                                        result_box.push(
                                            serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_series_result", uuid_boop), list.result_box).placeholder("Series Search Results")),
                                        )
                                    }
                                    message = format!("Found {} results", list.result_items);
                                }
                                Err(e) => {
                                    message = format!("Error searching for series: {}", e);
                                }
                            }
                        }
                        None => {
                        }
                    }
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(message).components(buttons.clone().iter().chain(result_box.iter()).cloned().collect())
                    ).await?;
                },
                Err(e) => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("Error Getting user data {}", e))
                    ).await?;
                }
            };
        }

        if send_final {
            mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?;
        }
    }

    Ok(())
}

async fn get_series(emby_client: &EmbyClient, series_name: &str) -> Result<EmbySearchResult, Error> {
    let series_result = match emby_client.search_series(series_name).await {
        Ok(d) => Ok(d),
        Err(e) => Err(Box::new(BotError::new(e.to_string().as_str())))
    }?;
    let menu_options: Vec<CreateSelectMenuOption> = series_result
      .iter()
      .map(|series| {
        CreateSelectMenuOption::new(series.name.as_str(), series.id.to_string())
      })
      .collect();
    let menu_item_count = menu_options.len();
    let row = serenity::CreateSelectMenuKind::String { options: menu_options };
    Ok( EmbySearchResult { result_box: row, result_items: menu_item_count} )
}

async fn get_now_playing(pipeline_ref: &PlayQueue) -> String {
    match pipeline_ref.get_current_item() {
        Some(i) => {
            i.name()
        }
        None => "No item playing".to_string()
    }
}

async fn get_seasons(emby_client: &EmbyClient, series_id: &str) -> Result<EmbySearchResult, Error> {
    let season_result = match emby_client.get_seasons_for_series(series_id).await {
        Ok(d) => Ok(d),
        Err(e) => Err(Box::new(BotError::new(e.to_string().as_str())))
    }?;
    let menu_options: Vec<CreateSelectMenuOption> = season_result
      .iter()
      .map(|season| {
        CreateSelectMenuOption::new(season.name.as_str(), season.id.to_string())
      })
      .collect();
    let menu_item_count = menu_options.len();
    let row = serenity::CreateSelectMenuKind::String { options: menu_options };
    Ok( EmbySearchResult { result_box: row, result_items: menu_item_count} )
}

fn generate_episode_name(episode: EmbyItemData) -> String {
    format!("S{}E{} - {}", episode.season_num.as_ref().unwrap(), episode.episode_num.as_ref().unwrap(), episode.name)
}

async fn get_episodes(emby_client: &EmbyClient, season_id: &str) -> Result<EmbySearchResult, Error> {
    let episode_result = match emby_client.get_episodes_for_season(season_id).await {
        Ok(d) => Ok(d),
        Err(e) => Err(Box::new(BotError::new(e.to_string().as_str())))
    }?;
    let menu_options: Vec<CreateSelectMenuOption> = episode_result
      .iter()
      .map(|episode| {
        match &episode.path {
            Some(_episode_path) => {
                let label = generate_episode_name(episode.clone());
                CreateSelectMenuOption::new(label, episode.id.as_str())
            }
            None => {
                CreateSelectMenuOption::new(format!("NOT FOUND: {}", episode.name.as_str()), episode.name.as_str())
            }
        }
      })
      .collect();
    let menu_item_count = menu_options.len();
    let row = serenity::CreateSelectMenuKind::String { options: menu_options };
    Ok( EmbySearchResult { result_box: row, result_items: menu_item_count} )
}

async fn get_queue_selector(pipeline_ref: &PlayQueue, prefix: &str) -> Vec<CreateActionRow> {
    let mut queue_items: Vec<CreateSelectMenuOption> = pipeline_ref.get_queue_items().iter()
      .map(|item| {
        CreateSelectMenuOption::new(item.name(), item.id())
      })
      .collect();
    let num_items = queue_items.len().clone();
    if num_items == 0 {
        queue_items = vec![CreateSelectMenuOption::new("No items in queue!", "empty")];
    }
    let result_box = vec![serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            format!("{}_queue_list", prefix),
            serenity::CreateSelectMenuKind::String { options: queue_items }
        ).placeholder(format!("{} Queue Items", num_items)))];
    result_box
}

async fn get_valid_deployments(
    api: Api<Deployment>
) -> Result<Vec<String>, Error> {
    let list_req = ListParams::default().labels("rustobot5000.managed=true");
    let mut deployment_list: Vec<String> = Vec::new();
    for dep in api.list(&list_req).await? {
        deployment_list.push(dep.metadata.name.expect("somehow deployment has no metadata.name"))
    }

    return Ok(deployment_list);
}

async fn restart_deployment(
    api: Api<Deployment>,
    deployment_name: String
) -> Result<(), Error> {
    match api.restart(&deployment_name).await {
        Ok(_r) => {
            Ok(())
        },
        Err(e) => {
            let error_msg = format!("Error restarting {}: {}", deployment_name, e);
            error!("{error_msg}");
            Err(Box::new(BotError::new(&error_msg)))
        }
    }
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx, .. } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {}", e)
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let default_rtmp_address = "rtmp://localhost:7788/live/livestream";
    let default_emby_url = "http://localhost:8096";

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let emby_api_token = std::env::var("EMBY_API_TOKEN").expect("missing EMBY_API_TOKEN");
    let emby_api_address = std::env::var("EMBY_API_URL").unwrap_or(default_emby_url.to_string());
    let rtmp_dst_address = std::env::var("RTMP_URI").unwrap_or(default_rtmp_address.to_string());

    let intents = serenity::GatewayIntents::non_privileged();
    // Bind the string to a variable so it isn't dropped immediately
    let guild_ids_str = std::env::var("DISCORD_SERVER_IDS").unwrap_or_else(|_| "1206408803118088202".to_string());
    let commands = vec![
        help(), 
        game_list(), 
        game_restart(), 
        game_status(),
        game_logs(),
        queue_video(),
        play_video(),
        stop_video(),
        pause_video(),
        rusto_player(),
    ];
    let play_queue = PlayQueue::new(&rtmp_dst_address).unwrap();
    let shared_play_queue = Arc::new(Mutex::new(play_queue));
    let main_playqueue = Arc::clone(&shared_play_queue.clone());
    let eos_watch_playqueue = Arc::clone(&shared_play_queue.clone());
    tokio::spawn(async move {
        PlayQueue::add_eos_watch(&eos_watch_playqueue).await;
    });
    let emby_client = EmbyClient::new(emby_api_address, emby_api_token).await.unwrap();
    tracing_subscriber::fmt::init();

    let guild_ids: Vec<_> = guild_ids_str.split(",")
        .map(|f| {
            f.parse::<u64>()
             .map(serenity::GuildId::new)
             .expect("invalid guild id")
        })
        .collect();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands,
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("~".into()),
                ..Default::default()
            },
            pre_command: |ctx| {
                Box::pin(async move {
                    let channel_name = &ctx.channel_id().name(&ctx).await.unwrap_or_else(|_| "<unknown>".to_string());
                    let author = &ctx.author().name;
                    info!(
						"user {} in channel {} used slash command '{}'",
						author,
						channel_name,
						&ctx.invoked_command_name()
					);
                })
            },
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                info!("Registering {} commands", &framework.options().commands.len());
                for guild_id in guild_ids {
                    info!("Registering in guild id {guild_id}");
                    poise::builtins::register_in_guild(ctx, &framework.options().commands, guild_id).await?;
                }
                let empty_commands = vec![help()];
                poise::builtins::register_globally(ctx, &empty_commands).await?;
                Ok(Data::load(ctx, main_playqueue, emby_client).await)
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .expect("error creating serenity client");
    let mut ctrl_c = signal(SignalKind::interrupt()).expect("failed to listen for interrupt");
    let mut sig_term = signal(SignalKind::terminate()).expect("failed to listen for SIGTERM");
    let mut sig_quit = signal(SignalKind::quit()).expect("failed to listen for SIGQUIT");
    tokio::select! {
        _ = client.start() => println!("Client stopped"),
        _ = ctrl_c.recv() => println!("Received Ctrl+C, shutting down..."),
        _ = sig_term.recv() => println!("Received SIGTERM, shutting down..."),
        _ = sig_quit.recv() => println!("Received SIGQUIT, shutting down..."),
    };
    client.shard_manager.shutdown_all().await;
    match shared_play_queue.clone().lock().await.stop_playback() {
        Ok(_) => (),
        Err(e) => error!("error stopping pipeline {}", e)
    }
}