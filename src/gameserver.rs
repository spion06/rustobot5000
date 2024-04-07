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
async fn status(
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
