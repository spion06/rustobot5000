use embyclient::{EmbyClient};
use k8s_openapi::api::apps::v1::Deployment;
use kube::{Api, Client as KubeClient};
use poise::{samples::HelpConfiguration, serenity_prelude::{self as serenity, CreateSelectMenuKind, CreateSelectMenuOption}, FrameworkError};
use std::{fmt, sync::Arc};
use tracing::{info, error};
use tokio::{signal::unix::{signal, SignalKind}, sync::{Mutex, MutexGuard}};
mod gstreamer;
mod embyclient;
use gstreamer::PlayQueue;
mod video_commands;
mod gameserver;
extern crate gstreamer as gst;

#[derive(Debug, poise::Modal)]
#[allow(dead_code)]
struct ShowSearch {
    show_name: String,
    search_type: String,
}

// Define a custom error type
#[derive(Debug)]
struct BotError {
    details: String,
}

#[derive(Clone)]
struct EmbySearchResult {
    result_menu_option: Vec<CreateSelectMenuOption>,
    result_items: usize,
}

impl EmbySearchResult {
    pub fn to_menu(&self) -> CreateSelectMenuKind {
        serenity::CreateSelectMenuKind::String { options: self.result_menu_option.clone()}
    }
    pub fn to_msg(&self, result_type: Option<&str>) -> String {
        let type_name = result_type.unwrap_or("items");
        format!("found {} {}", self.result_items, type_name)
    }
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
    Box::new(BotError::new(msg))
}

impl std::error::Error for BotError {}

/// Path remapping configuration for translating Emby paths to local paths
#[derive(Clone)]
pub(crate) struct PathRemap {
    pub from: String,
    pub to: String,
}

struct Data {
    video_pipeline: Arc<Mutex<PlayQueue>>,
    emby_client: Arc<EmbyClient>,
    path_remap: Option<PathRemap>,
} // User data, which is stored and accessible in all command invocations
impl Data {
    pub async fn load(_ctx: &serenity::Context, video_pipeline: Arc<Mutex<PlayQueue>>, emby_client: EmbyClient, path_remap: Option<PathRemap>) -> Self {
        Self {
            video_pipeline,
            emby_client: Arc::new(emby_client),
            path_remap,
        }
    }

    /// Remap a path using configured path remapping, if any
    pub fn remap_path(&self, path: &str) -> String {
        match &self.path_remap {
            Some(remap) => path.replace(&remap.from, &remap.to),
            None => path.to_string(),
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

#[poise::command(prefix_command, owners_only)]
async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

fn rusto_register() -> poise::Command<Data, Error> {
    poise::Command {
        name: "rusto_register".to_string(),
        description: Some("register commands".to_string()),
        slash_action: Some(|ctx| Box::pin(async move { 
            match poise::builtins::register_application_commands_buttons(ctx.into()).await {
                Ok(t) => Ok(t),
                Err(e) => Err(FrameworkError::new_command(ctx.into(), Box::new(e)))
            }
        })),
        ..Default::default()
    }
}


async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            error!("Failed to start bot: {:?}", error);
            std::process::exit(1);
        },
        poise::FrameworkError::Command { error, ctx, .. } => {
            error!("Error in command `{}`: {:?}", ctx.command().name, error);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                error!("Error while handling error: {}", e)
            }
        }
    }
}


#[tokio::main]
async fn main() {
    let default_rtmp_address = "rtmp://localhost:7788/live/livestream";
    let default_emby_url = "http://localhost:8096";

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN environment variable is required");
    let emby_api_token = std::env::var("EMBY_API_TOKEN").expect("EMBY_API_TOKEN environment variable is required");
    let emby_api_address = std::env::var("EMBY_API_URL").unwrap_or(default_emby_url.to_string());
    let rtmp_dst_address = std::env::var("RTMP_URI").unwrap_or(default_rtmp_address.to_string());

    // Optional path remapping for translating Emby paths to local filesystem paths
    let path_remap = match (std::env::var("PATH_REMAP_FROM"), std::env::var("PATH_REMAP_TO")) {
        (Ok(from), Ok(to)) => {
            info!("Path remapping enabled: '{}' -> '{}'", from, to);
            Some(PathRemap { from, to })
        }
        _ => None,
    };

    let intents = serenity::GatewayIntents::non_privileged();
    // Bind the string to a variable so it isn't dropped immediately
    let guild_ids_str = std::env::var("DISCORD_SERVER_IDS").unwrap_or_else(|_| "1206408803118088202".to_string());
    let commands = vec![
        help(), 
        register(),
        rusto_register(),
        gameserver::rusto_gameadmin(),
        video_commands::rusto_video(),
    ];
    let play_queue = match PlayQueue::new(&rtmp_dst_address) {
        Ok(pq) => pq,
        Err(e) => {
            eprintln!("Error: Failed to initialize GStreamer pipeline: {}", e);
            std::process::exit(1);
        }
    };
    let shared_play_queue = Arc::new(Mutex::new(play_queue));
    let main_playqueue = Arc::clone(&shared_play_queue.clone());
    let eos_watch_playqueue = Arc::clone(&shared_play_queue.clone());
    let eos_thread = tokio::spawn(async move {
        PlayQueue::add_eos_watch(&eos_watch_playqueue).await;
    });
    let emby_client = match EmbyClient::new(emby_api_address, emby_api_token).await {
        Ok(ec) => ec,
        Err(e) => {
            eprintln!("Error: Failed to initialize Emby client: {}", e);
            std::process::exit(1);
        }
    };
    tracing_subscriber::fmt::init();

    let guild_ids: Vec<_> = guild_ids_str.split(',')
        .filter_map(|f| {
            let trimmed = f.trim();
            match trimmed.parse::<u64>() {
                Ok(id) => Some(serenity::GuildId::new(id)),
                Err(e) => {
                    error!("Invalid guild ID '{}': {}, skipping", trimmed, e);
                    None
                }
            }
        })
        .collect();

    if guild_ids.is_empty() {
        eprintln!("Error: No valid guild IDs configured. Set DISCORD_SERVER_IDS environment variable.");
        std::process::exit(1);
    }

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands,
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
                Ok(Data::load(ctx, main_playqueue, emby_client, path_remap).await)
            })
        })
        .build();

    let mut client = match serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: Failed to create Discord client: {}", e);
                std::process::exit(1);
            }
        };
    let mut ctrl_c = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Failed to listen for SIGINT: {}", e);
            std::process::exit(1);
        }
    };
    let mut sig_term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Failed to listen for SIGTERM: {}", e);
            std::process::exit(1);
        }
    };
    let mut sig_quit = match signal(SignalKind::quit()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Failed to listen for SIGQUIT: {}", e);
            std::process::exit(1);
        }
    };
    tokio::select! {
        _ = client.start_autosharded() => println!("Client stopped"),
        _ = ctrl_c.recv() => println!("Received Ctrl+C, shutting down..."),
        _ = sig_term.recv() => println!("Received SIGTERM, shutting down..."),
        _ = sig_quit.recv() => println!("Received SIGQUIT, shutting down..."),
    };
    client.shard_manager.shutdown_all().await;
    eos_thread.abort();
    match shared_play_queue.clone().lock().await.stop_playback().await {
        Ok(_) => (),
        Err(e) => error!("error stopping pipeline {}", e)
    }
}
