use crate::{bot_error, embyclient::{EmbyClient, EmbyItemData, EmbySearch, SearchItemType}, gstreamer::PlayQueue, BotError, Context, EmbySearchResult, Error, ShowSearch};

use paginate::Pages;
use poise::{serenity_prelude::{self as serenity, ComponentInteractionDataKind, CreateActionRow, CreateAttachment, CreateSelectMenuKind, CreateSelectMenuOption}, CreateReply, Modal};
use strum::IntoEnumIterator;
use uuid::Uuid;
use std::str::FromStr;
use tracing::{info, error, warn};


#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", subcommands("add", "play", "pause", "stop", "skip", "list_series", "list_movies", "player", "seek"), subcommand_required)]
pub(crate) async fn rusto_video(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}


#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn add(
    ctx: Context<'_>,
    #[description = "path to a video to play"] url: String,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.add_uri(url.clone(), url.clone().split("/").last().unwrap().to_string(), None) {
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
async fn play(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.start_playback().await {
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
async fn stop(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.stop_playback().await {
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
async fn pause(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.pause_playback().await {
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
async fn skip(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.skip_video().await {
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

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn seek(
    ctx: Context<'_>,
    seek_seconds: i64,
) -> Result<(), Error> {
    let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
    match &pipeline_ref.seek_video(seek_seconds).await {
        Ok(pos) => {
            ctx.say(format!("seeked {}s to {}s", seek_seconds, pos)).await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("error seeking video: {}", e);
            ctx.say(err_msg.clone()).await?;
            error!(err_msg);
            Err(bot_error(err_msg.as_str()))
        }
    }
}

async fn get_buttons(interaction_prefix: String, user: &Option<EmbyItemData>, result_box: Option<Vec<CreateActionRow>>) -> Vec<CreateActionRow> {
    let user_button_label = match user {
        Some(u) => format!("User: {}", u.name),
        None => "User: (None)".to_string(),
    };
    let result_box = match result_box {
        Some(rb) => rb,
        None => vec![],
    };
    vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("{interaction_prefix}_play"))
                .style(serenity::ButtonStyle::Primary)
                .label("play")
                .emoji('\u{25B6}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_pause"))
                .style(serenity::ButtonStyle::Primary)
                .label("pause")
                .emoji('\u{23F8}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_stop"))
                .style(serenity::ButtonStyle::Primary)
                .label("stop")
                .emoji('\u{23F9}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_skip"))
                .style(serenity::ButtonStyle::Primary)
                .label("skip")
                .emoji('\u{23ED}'),
        ]),
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("{interaction_prefix}_search"))
                .style(serenity::ButtonStyle::Primary)
                .label("search")
                .emoji('\u{1F50D}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_show_queue"))
                .style(serenity::ButtonStyle::Primary)
                .label("queue")
                .emoji('\u{1F4DC}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_now_playing"))
                .style(serenity::ButtonStyle::Primary)
                .label("now playing")
                .emoji('\u{1F3A6}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_select_user"))
                .style(serenity::ButtonStyle::Primary)
                .label(user_button_label)
                .emoji('\u{1F9D4}'),
        ]),
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("{interaction_prefix}_seek_minus_300"))
                .style(serenity::ButtonStyle::Primary)
                .label("-5m")
                .emoji('\u{23EA}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_seek_minus_60"))
                .style(serenity::ButtonStyle::Primary)
                .label("-1m")
                .emoji('\u{23EA}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_seek_plus_60"))
                .style(serenity::ButtonStyle::Primary)
                .label("+1m")
                .emoji('\u{23E9}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_seek_plus_300"))
                .style(serenity::ButtonStyle::Primary)
                .label("+5m")
                .emoji('\u{23E9}'),
            serenity::CreateButton::new(format!("{interaction_prefix}_seek_plus_900"))
                .style(serenity::ButtonStyle::Primary)
                .label("+15m")
                .emoji('\u{23E9}'),
        ]),
    ].iter().chain(result_box.iter()).cloned().collect()
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn list_series(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let emby_client = ctx.data().emby_client.as_ref();
    let series_list = emby_client.get_all_series().await?.iter().map(|f| f.name.clone()).collect::<Vec<String>>().join("\n");
    let attachment_name = "all_series.csv";
    let attachment_logs = CreateAttachment::bytes(series_list.as_bytes(), attachment_name);
    ctx.send(CreateReply::default().attachment(attachment_logs)).await?;
    Ok(())
}

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn list_movies(
    ctx: Context<'_>,
) -> Result<(), Error> {
    let emby_client = ctx.data().emby_client.as_ref();
    let series_list = emby_client.get_all_movies().await?.iter().map(|f| f.name.clone()).collect::<Vec<String>>().join("\n");
    let attachment_name = "all_series.csv";
    let attachment_logs = CreateAttachment::bytes(series_list.as_bytes(), attachment_name);
    ctx.send(CreateReply::default().attachment(attachment_logs)).await?;
    Ok(())
}

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn player(ctx: Context<'_>) -> Result<(), Error> {
    // using ctx.id here prevents issues with multiple bot instances
    let interaction_prefix = ctx.id();
    let mut current_user = None;
    // current identifier to be used between iteractions
    let mut id_context: Option<String> = None;

    let reply = {
        CreateReply::default()
            .content("I want to watch something \u{1F346}")
            .components(get_buttons(interaction_prefix.to_string(), &current_user, None).await)
    };

    ctx.send(reply).await?;

    while let Some(mci) = serenity::ComponentInteractionCollector::new(ctx)
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .timeout(std::time::Duration::from_secs(3600))
        .filter(move |mci| mci.data.custom_id.starts_with(&interaction_prefix.clone().to_string()))
        .await
    {
        if ! mci.data.custom_id.starts_with(interaction_prefix.to_string().as_str()) {
            return Ok(())
        }
        let mut send_final = true;
        let mut msg = mci.message.clone();
        let mut pipeline_ref = ctx.data().get_pipeline_ref().await;
        if mci.data.custom_id.ends_with("play") {
            match &pipeline_ref.start_playback().await {
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
            match &pipeline_ref.pause_playback().await {
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
            match &pipeline_ref.stop_playback().await {
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
            match &pipeline_ref.skip_video().await {
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
        if mci.data.custom_id.contains("_seek_") {
            let parts: Vec<&str> = mci.data.custom_id.split('_').collect();
            let numeric_parts: i64 = match parts.last() {
                Some(&num_str) => {
                    match num_str.parse() {
                        Ok(num) => num,
                        Err(_) => 0,
                    }
                },
                None => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("error getting seek amount"))
                    ).await?;
                    0 as i64
                }
            };

            let numeric_sign: i64 = match parts.get(parts.len() - 2) {
                Some(&sign) => {
                    if sign == "minus" {
                        -1
                    } else {
                        1
                    }
                }
                None => {
                    msg.edit(
                        ctx,
                        serenity::EditMessage::new().content(format!("error getting seek sign"))
                    ).await?;
                    0
                }
            };

            let seek_amount = numeric_sign * numeric_parts;

            if numeric_parts != 0 {
                let response = match pipeline_ref.seek_video(seek_amount).await {
                    Ok(dst_ts) => {
                        format!("seeked to {}s", dst_ts)
                    }
                    Err(e) => {
                        format!("Error seeking {}", e)
                    }
                };
                msg.edit(
                    ctx,
                    serenity::EditMessage::new().content(response)
                ).await?;
            }

        }
        if mci.data.custom_id.ends_with("show_queue") {
            let result_box = get_queue_selector(&pipeline_ref, interaction_prefix.to_string().as_str()).await;
            msg.edit(
                ctx,
                serenity::EditMessage::new().components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
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
                let result_box = get_queue_selector(&pipeline_ref, interaction_prefix.to_string().as_str()).await;
                msg.edit(
                    ctx,
                    serenity::EditMessage::new().components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
                ).await?;
            }
        }

        // handle result from clicking on a series
        if mci.data.custom_id.ends_with("first_item_result") {
            let item_id_w_type = match &mci.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => &values[0],
                k => {
                    warn!("got an unknown selection kind on series {:#?}", k);
                    "unknown"
                }
            };
            let parts = item_id_w_type.split("_").into_iter().map(|t| t.to_string()).collect::<Vec<String>>();
            let result_type = match parts.get(0) {
                Some(p) => p.clone(),
                None => "unknown".to_string(),
            };
            let result_id = match parts.get(1) {
                Some(p) => p.clone(),
                None => "unknown".to_string(),
            };
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(format!("Got {} {}", result_type, result_id))
            ).await?;
            let mut result_box: Vec<CreateActionRow> = vec![];
            let mut message: String = "No results found".to_string();
            match result_type.as_str() {
                "series" => {
                    match get_seasons(ctx.data().emby_client.as_ref(), &result_id).await {
                        Ok(seasons) => {
                            result_box.push(
                                serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_season_result", interaction_prefix), seasons.to_menu()).placeholder(format!("{} Seasons", seasons.result_items))),
                            );
                            message = format!("Found {} Seasons", seasons.result_items);
                        }
                        Err(e) => {
                            message = format!("Error getting seasons: {}", e);
                        }
                    }
                }
                "movie" => {
                    message = add_emby_item(ctx, &mut pipeline_ref, &result_id, &current_user).await?
                }
                v => {
                    message = format!("unknown item {}", v)
                }
            }
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message).components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
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
            let (result_box, message) = handle_episode_search(interaction_prefix.to_string(), season_id, &current_user, ctx, 1).await;
            id_context = Some(season_id.to_string());
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message).components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
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

            if episode_id.starts_with("page_") {
                let extracted_page = episode_id.split("_").last();
                match extracted_page {
                    Some(p) => {
                        let page_num: u32 = p.parse().expect("unable to parse page number");
                        match id_context.clone() {
                            Some(season_id) => {
                                let (result_box, message) = handle_episode_search(interaction_prefix.to_string(), season_id.as_str(), &current_user, ctx, page_num).await;
                                msg.edit(
                                    ctx,
                                    serenity::EditMessage::new().content(message).components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
                                ).await?;
                            },
                            None => {
                                message = "no season id found when getting next page! this is a bug!".to_string();
                                error!(message);
                                msg.edit(
                                    ctx,
                                    serenity::EditMessage::new().content(message)
                                ).await?
                            }
                        }
                    }
                    None => {
                        message = "could not extract page id! this is a bug!".to_string();
                        error!(message);
                        msg.edit(
                            ctx,
                            serenity::EditMessage::new().content(message)
                        ).await?
                    }
                }
            } else {
                let message = add_emby_item(ctx, &mut pipeline_ref, episode_id, &current_user).await?;
                msg.edit(
                    ctx,
                    serenity::EditMessage::new().content(message)
                ).await?;
            }
        }

        // handle result from clicking on select user
        if mci.data.custom_id.ends_with("select_user") {
            msg.edit(
                ctx,
                serenity::EditMessage::new().content("Select a user")
            ).await?;
            let mut result_box: Vec<CreateActionRow> = vec![];
            let mut message: String = "No results found".to_string();
            match get_users(ctx.data().emby_client.as_ref()).await {
                Ok(seasons) => {
                    result_box.push(
                        serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_user_list_result", interaction_prefix), seasons.to_menu()).placeholder(format!("{} Users", seasons.result_items))),
                    );
                    message = seasons.to_msg(Some("User"));
                    info!(message);
                }
                Err(e) => {
                    message = format!("Error getting users: {}", e);
                    error!(message);
                }
            }
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message).components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
            ).await?;
        }

        // handle result from clicking on select user
        if mci.data.custom_id.ends_with("user_list_result") {
            let user_id = match &mci.data.kind {
                ComponentInteractionDataKind::StringSelect { values } => &values[0],
                _ => {
                    warn!("got an unknown selection kind on users");
                    "unknown"
                }
            };
            let mut message: String = "No results found".to_string();
            let user_name = "";
            if user_id == "None" {
                current_user = None;
            } else {
                current_user = Some(ctx.data().emby_client.as_ref().get_user_by_id(user_id.to_string()).await?);
            };
            message = format!("Set user to {}", user_name);
            msg.edit(
                ctx,
                serenity::EditMessage::new().content(message).components(get_buttons(interaction_prefix.to_string(), &current_user, None).await)
            ).await?;
        }

        if mci.data.custom_id.ends_with("search") {
            // this will block until a user respons and prevent 
            msg.edit(
                ctx,
                serenity::EditMessage::new().content("Waiting for user input...")
            ).await?;
            let default_input = ShowSearch {
                search_type: SearchItemType::iter().map(|i| i.to_string()).collect::<Vec<String>>().join(","),
                show_name: "".to_string(),
            };
            let data = poise::execute_modal_on_component_interaction::<ShowSearch>(ctx, mci.clone(), Some(default_input), Some(std::time::Duration::from_secs(30))).await;
            let mut result_box: Vec<CreateActionRow> = vec![];
            let mut message: String = "No results or input timeout found".to_string();
            match &data {
                Ok(d) => {
                    send_final = false;
                    match d {
                        Some(user_search) => {
                            let mut search_types = vec![];
                            for s_type in user_search.search_type.split(",") {
                                match SearchItemType::from_str(s_type) {
                                    Ok(v) => search_types.push(v),
                                    Err(e) => error!("invalid search item type {}: {}", s_type, e)
                                }
                            }
                            match get_items(ctx.data().emby_client.as_ref(), &user_search.show_name, search_types).await {
                                Ok(list) => {
                                    if list.result_items == 0 {
                                        let empty_result = CreateSelectMenuKind::String { options: vec![CreateSelectMenuOption::new("No Results found!", "empty")] };
                                        result_box.push(
                                            serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_first_item_result", interaction_prefix), empty_result).placeholder("Search Results")),
                                        )
                                    } else {
                                        result_box.push(
                                            serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_first_item_result", interaction_prefix), list.to_menu()).placeholder("Search Results")),
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
                        serenity::EditMessage::new().content(message).components(get_buttons(interaction_prefix.to_string(), &current_user, Some(result_box)).await)
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

async fn add_emby_item(ctx: Context<'_>, pipeline_ref: &mut PlayQueue, item_id: &str, current_user: &Option<EmbyItemData>) -> Result<String, Error> {
    let mut message = "nothing".to_string();
    let episode_info = ctx.data().emby_client.as_ref().get_item_info(item_id).await?;
    let episode_path = match episode_info.clone().path {
        Some(path) => path,
        None => "".to_string(),
    };
    if episode_path.is_empty() {
        message = format!("could not find episode info for {}", episode_info.name);
        error!(message)
    } else {
        info!("Got episode {}", episode_path);
        let episode_path = episode_path.replace("/mnt/storage", "/mnt/zfspool/storage");
        let stop_fn = match &current_user {
            Some(u) => Some(ctx.data().emby_client.as_ref().user_stop_fn(u.id.clone(), episode_info.id.clone()).await),
            None => None,
        };
        match pipeline_ref.add_uri(episode_path.to_string(), generate_episode_name(episode_info.clone()), stop_fn) {
            Ok(i) => {
                message = format!("added {} to queue", i.name());
            }
            Err(e) => {
                message = format!("error adding {} to queue: {}", episode_path, e);
                error!(message)
            }
            
        };
    };
    Ok(message.to_string())
}

async fn get_items(emby_client: &EmbyClient, item_name: &str, item_types: Vec<SearchItemType>) -> Result<EmbySearchResult, Error> {
    let series_result = if item_name == "all" {
        match emby_client.get_all_series().await {
            Ok(d) => Ok(d),
            Err(e) => Err(Box::new(BotError::new(e.to_string().as_str())))
        }?
    } else {
        match emby_client.search_items(item_name, item_types).await {
            Ok(d) => Ok(d),
            Err(e) => Err(Box::new(BotError::new(e.to_string().as_str())))
        }?
    };
    let menu_options: Vec<CreateSelectMenuOption> = series_result
      .iter()
      .map(|series| {
        let item_type = series.item_type.clone().unwrap_or("Unknown".to_string());
        let (label_prefix, value_prefix) = match item_type.as_str() {
            "Movie" => ("\u{1F4FD}", "movie"),
            "Series" => ("\u{1F4FA}", "series"),
            _ => ("unknown: ", "unknown"),
        };
        CreateSelectMenuOption::new(format!("{}: {}", label_prefix, series.name.as_str()), format!("{}_{}", value_prefix, series.id.to_string()))
      })
      .collect();
    let menu_item_count = menu_options.len();
    info!("found {} series", menu_item_count.clone());
    Ok( EmbySearchResult { result_menu_option: menu_options, result_items: menu_item_count} )
}

async fn get_users(emby_client: &EmbyClient) -> Result<EmbySearchResult, Error> {
    let users = emby_client.get_users().await?;
    let menu_options: Vec<CreateSelectMenuOption> = users
      .iter()
      .map(|user| {
        CreateSelectMenuOption::new(user.name.to_string(), user.id.to_string())
      })
      .collect();
    let menu_options: Vec<CreateSelectMenuOption> = vec![CreateSelectMenuOption::new("None", "None")].iter().chain(menu_options.iter()).cloned().collect();
    let menu_item_count = menu_options.len();
    info!("found {} users", menu_item_count.clone());
    Ok( EmbySearchResult { result_menu_option: menu_options, result_items: menu_item_count} )
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
    Ok( EmbySearchResult { result_menu_option: menu_options, result_items: menu_item_count} )
}

fn generate_episode_name(episode: EmbyItemData) -> String {
    let watched_icon = match episode.user_data {
        Some(u) => {
            if u.played {
                format!("{}: ", '\u{1F7E2}')
            } else {
                format!("{}: ", '\u{1F534}')
            }
        }
        None => "".to_string(),
    };
    if episode.item_type.unwrap_or("Unknown".to_string()) == "Movie" {
        format!("{}Movie - {}", watched_icon, episode.name)
    } else {
        format!("{}S{}E{} - {}", watched_icon, episode.season_num.as_ref().unwrap(), episode.episode_num.as_ref().unwrap(), episode.name)
    }
}

fn paginate_result(search_result: EmbySearchResult, page_number: u32) -> Result<EmbySearchResult, Error> {
    let page_number_idx = if page_number > 0 {
        page_number - 1
    } else {
        page_number
    };
    if search_result.result_items > 25 {
        // 23 pages so there is an item for previous/next page
        let pages = Pages::new(search_result.result_items, 23);
        let mut menu_options: Vec<CreateSelectMenuOption> = vec![];
        if page_number > 1 {
            let prev_page = page_number - 1;
            menu_options.push(CreateSelectMenuOption::new(format!("Back Page: {}", prev_page), format!("page_{}", prev_page)));
        }
        if page_number as usize > pages.page_count() {
            return Err(Box::new(BotError::new(format!("Page {} is outside of the valid max of {}", page_number, pages.page_count()).as_str())))
        }
        let page = pages.with_offset(page_number_idx as usize);
        for idx in page.start..=page.end {
            let option = search_result.result_menu_option.get(idx);
            match option {
               Some(i) => menu_options.push(i.clone()),
               None => { return Err(Box::new(BotError::new("unable to get page item. out of range"))) }
            }
        }
        if page_number as usize != pages.page_count() {
            let next_page = page_number + 1;
            menu_options.push(CreateSelectMenuOption::new(format!("Next Page: {}", next_page), format!("page_{}", next_page)));
        }

        let result = EmbySearchResult { result_menu_option: menu_options, result_items: search_result.result_items};

        Ok(result)
    } else {
        return Ok(search_result)
    }
}

async fn handle_episode_search(interaction_prefix: String, season_id: &str, current_user: &Option<EmbyItemData>, ctx: Context<'_>, page_number: u32) -> (Vec<CreateActionRow>, String) {
    let mut message: String = "no result found".to_string();
    let mut result_box: Vec<CreateActionRow> = vec![];
    match get_episodes(ctx.data().emby_client.as_ref(), season_id, &current_user).await {
        Ok(episodes) => {
            let paged_result = paginate_result(episodes, page_number).expect("Unable to paginate result");
            result_box.push(
                serenity::CreateActionRow::SelectMenu(serenity::CreateSelectMenu::new(format!("{}_episodes_result", interaction_prefix), paged_result.to_menu()).placeholder(format!("{} Series Episodes", paged_result.result_items))),
            );
            message = paged_result.to_msg(Some("episodes"));
        }
        Err(e) => {
            message = format!("Error getting episodes: {}", e);
        }
    }
    return (result_box, message);
}

async fn get_episodes(emby_client: &EmbyClient, season_id: &str, current_user: &Option<EmbyItemData>) -> Result<EmbySearchResult, Error> {
    let episode_result = match emby_client.get_episodes_for_season(season_id, current_user).await {
        Ok(d) => Ok(d),
        Err(e) => Err(Box::new(BotError::new(e.to_string().as_str())))
    }?;
    let menu_options: Vec<CreateSelectMenuOption> = episode_result
      .iter()
      .map(|episode| {
        match &episode.path {
            Some(_episode_path) => {
                let mut label = generate_episode_name(episode.clone());
                // label must be under 25 characters so truncate the label name
                label.truncate(64);
                CreateSelectMenuOption::new(label, episode.id.as_str())
            }
            None => {
                CreateSelectMenuOption::new(format!("NOT FOUND: {}", episode.name.as_str()), episode.name.as_str())
            }
        }
      })
      .collect();
    let menu_item_count = menu_options.len();
    Ok( EmbySearchResult { result_menu_option: menu_options, result_items: menu_item_count} )
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