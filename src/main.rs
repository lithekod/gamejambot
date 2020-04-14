use std::{env, error::Error};
use tokio::stream::StreamExt;

use twilight::{
    cache::{
        twilight_cache_inmemory::config::{InMemoryConfigBuilder, EventType},
        InMemoryCache,
    },
    gateway::cluster::{config::ShardScheme, Cluster, ClusterConfig},
    gateway::shard::Event,
    http::Client as HttpClient,
    model::{
        gateway::GatewayIntents,
        user::CurrentUser,
        channel::Channel,
        id::ChannelId,
    },
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let token = env::var("DISCORD_TOKEN")?;

    // This is also the default.
    let scheme = ShardScheme::Auto;

    let config = ClusterConfig::builder(&token)
        .shard_scheme(scheme)
        // Use intents to only listen to GUILD_MESSAGES events
        .intents(Some(
            GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES,
        ))
        .build();

    // Start up the cluster
    let cluster = Cluster::new(config);
    cluster.up().await?;

    // The http client is seperate from the gateway,
    // so startup a new one
    let http = HttpClient::new(&token);

    // Since we only care about messages, make the cache only
    // cache message related events
    let cache_config = InMemoryConfigBuilder::new()
        .event_types(
            EventType::MESSAGE_CREATE
                | EventType::MESSAGE_DELETE
                | EventType::MESSAGE_DELETE_BULK
                | EventType::MESSAGE_UPDATE,
        )
        .build();
    let cache = InMemoryCache::from(cache_config);


    let mut events = cluster.events().await;

    let current_user = http.current_user().await?;
    // Startup an event loop for each event in the event stream
    while let Some(event) = events.next().await {
        // Update the cache
        cache.update(&event.1).await.expect("Cache failed, OhNoe");

        // Spawn a new task to handle the event
        handle_event(event, http.clone(), &current_user).await?;
    }

    Ok(())
}

async fn is_pm(http: &HttpClient, channel_id: ChannelId) -> Result<bool> {
    match http.channel(channel_id).await?.unwrap() {
        Channel::Private(_) => Ok(true),
        _ => Ok(false)
    }
}

async fn handle_event(
    event: (u64, Event),
    http: HttpClient,
    current_user: &CurrentUser
) -> Result<()> {
    match event {
        (_, Event::MessageCreate(msg)) => {
            // Don't send replies to yourself
            if msg.author.id != current_user.id {
                if is_pm(&http, msg.channel_id).await? {
                    // Check if the message is a single word
                    if msg.content.split_ascii_whitespace().count() != 1 {
                        http.create_message(msg.channel_id)
                            .content("Themes ideas should only be a single word")
                            .await?;
                    }
                    else {
                        // Check if the message is a PM
                        http.create_message(msg.channel_id)
                            .content("Theme idea registered, thanks!")
                            .await?;
                    }
                }
                else {
                    http.create_message(msg.channel_id)
                        .content("not a pm")
                        .await?;
                }
            }
        }
        (id, Event::ShardConnected(_)) => {
            println!("Connected on shard {}", id);
        }
        _ => {}
    }

    Ok(())
}
