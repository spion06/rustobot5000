use crate::{BotError, Context, Error};
use poise::{serenity_prelude::CreateAttachment, CreateReply};
use kube::{ api::{ListParams, LogParams}, Api, Client as KubeClient};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Pod};
use tracing::{info, error, warn};

#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR", subcommands("list", "restart", "status", "logs"), subcommand_required)]
pub(crate) async fn rusto_gameadmin(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn validate_game_name(ctx: Context<'_>, game: String) -> Result<(), Error> {
    match ctx.data().get_deployment_client().await {
        Ok(client) => {
            let valid_deployments = get_valid_deployments(client).await?;
            if valid_deployments.contains(&game) {
                info!("{game} is a valid game name");
                Ok(())
            } else {
                info!("{game} is not a valid game name");
                Err(Box::new(BotError::new(&format!("{game} is not a valid game name"))))
            }
        },
        Err(e) => Err(e)
    }
}

/// list all the available games to restart
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn list(
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
async fn restart(
    ctx: Context<'_>,
    #[description = "Game to restart"] game: String,
) -> Result<(), Error> {
    validate_game_name(ctx, game.clone()).await?;
    match ctx.data().get_deployment_client().await {
        Ok(client) => {
            restart_deployment(client.clone(), game.clone()).await?;
            ctx.say(format!("Started restart on {game}")).await?;
            ctx.say("Check status with game_status command").await?;
            Ok(())
        },
        Err(e) => {
            let err_msg = format!("Error getting client: {e}");
            error!("{err_msg}");
            Err(e)
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
    let spec = deployment.spec.ok_or_else(|| Box::new(BotError::new(&format!("Deployment {} has no spec", deployment_name))))?;
    let match_labels = spec.selector.match_labels.ok_or_else(|| Box::new(BotError::new(&format!("Deployment {} has no match labels", deployment_name))))?;
    let selector_query = match_labels.iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>()
        .join(",");
    let lp = ListParams::default().labels(&selector_query);
    let pods = pod_client.list(&lp).await?;
    Ok(pods.items)
}

/// get the current status of a game. should be in running for "normal" operation
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
async fn status(
    ctx: Context<'_>,
    #[description = "Game to restart"] game: String,
) -> Result<(), Error> {
    validate_game_name(ctx, game.clone()).await?;
    match ctx.data().get_kube_client().await {
        Ok(kclient) => {
            let d_client: Api<Deployment> = Api::default_namespaced(kclient.clone());
            let resp = d_client.get_status(&game).await?;
            let status = match resp.status {
                Some(s) => s,
                None => {
                    ctx.say(format!("No status available for deployment {game}")).await?;
                    return Ok(());
                }
            };
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
                let pod_status = pod.status
                    .and_then(|s| s.phase)
                    .unwrap_or_else(|| "unknown".to_string());
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
async fn logs(
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
                let pod_name = match &pod.metadata.name {
                    Some(name) => name.clone(),
                    None => {
                        warn!("Pod has no name, skipping");
                        continue;
                    }
                };
                let log_params = LogParams {
                    tail_lines: Some(tail_lines),
                    ..LogParams::default()
                };
                info!("getting last {tail_lines} lines from {game}");
                let pod_logs = pod_client.logs(&pod_name, &log_params).await?;
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

async fn get_valid_deployments(
    api: Api<Deployment>
) -> Result<Vec<String>, Error> {
    let list_req = ListParams::default().labels("rustobot5000.managed=true");
    let mut deployment_list: Vec<String> = Vec::new();
    for dep in api.list(&list_req).await? {
        if let Some(name) = dep.metadata.name {
            deployment_list.push(name);
        } else {
            warn!("Found deployment without a name, skipping");
        }
    }

    Ok(deployment_list)
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
