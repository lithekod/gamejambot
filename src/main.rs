use std::env;

use tokio::stream::StreamExt;
use twilight::{
    cache::{
        twilight_cache_inmemory::config::{EventType, InMemoryConfigBuilder},
        InMemoryCache,
    },
    gateway::cluster::{config::ShardScheme, Cluster, ClusterConfig},
    gateway::shard::Event,
    http::Client as HttpClient,
    model::{
        channel::{Channel, Message},
        gateway::GatewayIntents,
        id::{ChannelId, GuildId, UserId},
        user::CurrentUser,
    },
};

mod channel;
mod reaction;
mod role;
mod roles;
mod state;
mod theme;
mod utils;

use channel::{handle_create_channels, handle_remove_channels, handle_rename_channels};
use reaction::{handle_reaction_add, handle_reaction_remove, handle_set_reaction_message, ReactionMessageType};
use role::{handle_give_role, handle_remove_role, has_role};
use roles::{JAMMER, ORGANIZER};
use theme::{handle_add_theme, handle_generate_theme, handle_show_all_themes};
use utils::{Result, send_message};

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
            GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::GUILD_MESSAGE_REACTIONS,
        ))
        .build();

    // Start up the cluster
    let cluster = Cluster::new(config);
    cluster.up().await?;

    // The http client is seperate from the gateway,
    // so startup a new one
    let http = HttpClient::new(&token);

    // Since we only care about messages and reactions, make
    // the cache only cache message and reaction related events
    let cache_config = InMemoryConfigBuilder::new()
        .event_types(
            EventType::MESSAGE_CREATE
                | EventType::MESSAGE_DELETE
                | EventType::MESSAGE_DELETE_BULK
                | EventType::MESSAGE_UPDATE
                | EventType::REACTION_ADD
                | EventType::REACTION_REMOVE,
        )
        .build();
    let cache = InMemoryCache::from(cache_config);


    let mut events = cluster.events().await;

    let current_user = http.current_user().await?;
    // Startup an event loop for each event in the event stream
    while let Some(event) = events.next().await {
        // Update the cache
        cache.update(&event.1).await.expect("Cache failed, OhNoe!");

        // Spawn a new task to handle the event
        handle_event(event, http.clone(), &current_user).await?;
    }

    Ok(())
}

/// Checks if the specified channel is a private message channel
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
                    handle_pm(&msg, &http).await?;
                }
                else {
                    handle_potential_command(&msg, http, current_user)
                        .await?;
                }
            }
        }
        (_, Event::ReactionAdd(reaction)) => {
            if !is_pm(&http, reaction.channel_id).await? {
                handle_reaction_add(&reaction, http, &current_user).await?;
            }
        }
        (_, Event::ReactionRemove(reaction)) => {
            if !is_pm(&http, reaction.channel_id).await? {
                handle_reaction_remove(&reaction, http).await?;
            }
        }
        (id, Event::ShardConnected(_)) => {
            println!("Connected on shard {}", id);
        }
        _ => {}
    }

    Ok(())
}


async fn handle_pm(
    msg: &Message,
    http: &HttpClient,
) -> Result<()> {
    handle_add_theme(http, msg).await?;
    Ok(())
}

async fn handle_potential_command(
    msg: &Message,
    http: HttpClient,
    current_user: &CurrentUser
) -> Result<()> {
    let mut words = msg.content.split_ascii_whitespace();
    match words.next() {
        Some("!help") => {
            send_help_message(
                http,
                msg.channel_id,
                msg.author.id,
                msg.guild_id.expect("Tried to call for help in non-guild"),
            ).await?;
        }
        Some("!createchannels") => {
            handle_create_channels(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to create channels in non-guild"),
                msg.author.id,
                current_user.id,
                http
            ).await?;
        },
        Some("!renamechannels") => {
            handle_rename_channels(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to remove channels in non-guild"),
                msg.author.id,
                current_user.id,
                http
            ).await?;
        },
        Some("!removechannels") => {
            handle_remove_channels(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to remove channels in non-guild"),
                msg.author.id,
                http
            ).await?;
        },
        Some("!role") => {
            handle_give_role(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to get role in non-guild"),
                &msg.author,
                http
            ).await?;
        },
        Some("!leave") => {
            handle_remove_role(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to leave role in non-guild"),
                &msg.author,
                http
            ).await?;
        },
        Some("!generatetheme") => {
            handle_generate_theme(
                msg.channel_id,
                msg.guild_id.expect("Tried to generate theme in non-guild"),
                &msg.author,
                http
            ).await?;
        }
        Some("!showallthemes") => {
            handle_show_all_themes(
                msg.channel_id,
                msg.guild_id.expect("Tried to show all themes in non-guild"),
                &msg.author,
                http
            ).await?;
        }
        Some("!seteula") => {
            handle_set_reaction_message(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to set EULA in non-guild"),
                &msg.author,
                http,
                msg,
                ReactionMessageType::Eula,
            ).await?;
        }
        Some("!setroleassign") => {
            handle_set_reaction_message(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to set role assignment message in non-guild"),
                &msg.author,
                http,
                msg,
                ReactionMessageType::RoleAssign,
            ).await?;
        }
        Some(s) if s.chars().next() == Some('!') => {
            send_message(&http, msg.channel_id, msg.author.id,
                format!("Unrecognised command `{}`.", s)
            ).await?;
            send_help_message(
                http,
                msg.channel_id,
                msg.author.id,
                msg.guild_id.expect("Tried to issue a command in non-guild"),
            ).await?;
        }
        // Not a command and probably not for us
        Some(_) => {
            // Check if we were mentioned
            if msg.mentions.contains_key(&current_user.id) {
                send_help_message(
                    http,
                    msg.channel_id,
                    msg.author.id,
                    msg.guild_id.expect("Tried to mention us in non-guild"),
                ).await?;
            }
        }
        None => {}
    }
    Ok(())
}

async fn send_help_message(
    http: HttpClient,
    channel_id: ChannelId,
    user_id: UserId,
    guild_id: GuildId,
) -> Result<()> {
    let standard_message =
        "Send me a PM to submit theme ideas.\n\n\
        Get a role to signify one of your skill sets with the command `!role <role name>`\n\
        and leave a role with `!leave <role name>`.";
    let jammer_message =
        "You can also ask for text and voice channels for your game \
        with the command `!createchannels <game name>`\n\
        and rename them with `!renamechannels <new game name>`.";
    let organizer_message = format!(
        "Since you have the **{}** role, you also have access to the \
        following commands:\n\
        - `!generatetheme` to generate a theme.\n\
        - `!showallthemes` to view all the theme ideas that have been submitted.\n\
        - `!removechannels <mention of user>` to remove a user's created channel.\n\
        - `!seteula <mention of channel with the message> <message ID>` to \
        set the message acting as the server's EULA.\n\
        - `!setroleassign <mention of channel with the message> <message ID>` to \
        set the server's role assignment message.", ORGANIZER
    );
    let help_message =
    if has_role(&http, guild_id, user_id, ORGANIZER).await? {
        format!("{}\n\n{}\n\n{}", standard_message, jammer_message, organizer_message)
    }
    else if has_role(&http, guild_id, user_id, JAMMER).await? {
        format!("{}\n\n{}", standard_message, jammer_message)
    }
    else {
        standard_message.to_string()
    };
    send_message(&http, channel_id, user_id, help_message).await?;
    Ok(())
}
