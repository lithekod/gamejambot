use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::sync::Mutex;
use std::collections::{HashSet, HashMap};
use std::path::PathBuf;

use tokio::stream::StreamExt;
use serde_derive::{Serialize, Deserialize};
use anyhow::Context;
use lazy_static::lazy_static;
use serde_json;

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
        channel::{Message, Channel, ChannelType, GuildChannel},
        id::{ChannelId, UserId, GuildId},
    },
};

type Result<T> = std::result::Result<T, anyhow::Error>;

enum SubmissionResult {
    Done,
    AlreadySubmitted,
}

const FILENAME: &'static str = "state.json";

#[derive(Serialize, Deserialize)]
struct PersistentState {
    theme_ideas: HashMap<UserId, String>,
    channel_creators: HashSet<UserId>
}

impl PersistentState {
    fn load() -> Result<Self> {
        if PathBuf::from(FILENAME).exists() {
            let mut file = File::open(FILENAME)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            Ok(serde_json::from_str(&content)?)
        }
        else {
            Ok(Self {
                theme_ideas: HashMap::new(),
                channel_creators: HashSet::new(),
            })
        }
    }

    pub fn instance() -> &'static Mutex<Self> {
        lazy_static! {
            static ref INSTANCE: Mutex<PersistentState> = Mutex::new(
                PersistentState::load().unwrap()
            );
        }
        &INSTANCE
    }

    pub fn try_add_theme(
        &mut self,
        user: UserId,
        idea: &str
    ) -> Result<SubmissionResult> {
        if self.theme_ideas.contains_key(&user) {
            self.theme_ideas.insert(user, idea.into());
            self.save().context("Failed to write current themes")?;
            Ok(SubmissionResult::AlreadySubmitted)
        }
        else {
            self.theme_ideas.insert(user, idea.into());
            self.save().context("Failed to write current themes")?;
            Ok(SubmissionResult::Done)
        }
    }

    pub fn is_allowed_channel(&mut self, id: UserId) -> bool {
        !self.channel_creators.contains(&id)
    }

    pub fn register_channel_creation(&mut self, id: UserId) -> Result<()> {
        self.channel_creators.insert(id);
        self.save()
    }

    pub fn save(&self) -> Result<()> {
        let mut file = File::create(FILENAME)
            .with_context(|| format!("failed to open {} for writing", FILENAME))?;
        file.write_all(serde_json::to_string(&self)?.as_bytes())
            .with_context(|| format!("failed to write to {}", FILENAME))?;
        Ok(())
    }
}

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
                        let has_old_theme = PersistentState::instance().lock().unwrap()
                            .try_add_theme(msg.author.id, &msg.content)
                            .context("failed to save theme")?;

                        match has_old_theme {
                            SubmissionResult::Done => {
                                // Check if the message is a PM
                                http.create_message(msg.channel_id)
                                    .content("Theme idea registered, thanks!")
                                    .await?;
                            }
                            SubmissionResult::AlreadySubmitted => {
                                // Check if the message is a PM
                                http.create_message(msg.channel_id)
                                    .content("You can only send one idea. We replaced your old submission")
                                    .await?;
                            }
                        }
                    }
                }
                else {
                    handle_potential_command(&msg, http, current_user)
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

async fn handle_potential_command(
    msg: &Message,
    http: HttpClient,
    current_user: &CurrentUser
) -> Result<()> {
    let mut words = msg.content.split_ascii_whitespace();
    match words.next() {
        Some("~help") => {
            send_help_message(msg.channel_id, http).await?;
        }
        Some("~create_team_channels") => {
            handle_create_team_channels(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to create channel in non-guild"),
                msg.author.id,
                http
            ).await?;
        },
        Some(s) if s.chars().next() == Some('~') => {
            http.create_message(msg.channel_id)
                .content("Unrecognised command")
                .await?;
            send_help_message(msg.channel_id, http).await?;
        }
        // Not a command and probably not for us
        Some(_) => {
            // Check if we were mentioned
            if msg.mentions.contains_key(&current_user.id) {
                send_help_message(msg.channel_id, http).await?;
            }
        }
        None => {}
    }
    Ok(())
}

async fn send_help_message(
    channel_id: ChannelId,
    http: HttpClient,
) -> Result<()> {
    http.create_message(channel_id)
        .content("Talk to me in a PM to submit theme ideas.\n\nYou can also ask for a voice channel by sending `~create_team_channels <channel name>`")
        .await?;
    Ok(())
}


async fn handle_create_team_channels<'a>(
    rest_command: &[&'a str],
    original_channel: ChannelId,
    guild: GuildId,
    user: UserId,
    http: HttpClient
) -> Result<()> {
    if !PersistentState::instance().lock().unwrap().is_allowed_channel(user) {
        http.create_message(original_channel)
            .content("You already created a channel")
            .await?;
        return Ok(())
    }

    let team_name = &*rest_command.join(" ");
    println!("got request for channel with name {:?}", team_name);
    let reply = if rest_command.len() == 0 {
        "You need to specify a team name".to_string()
    }
    else {
        // Category
        let category_request = http.create_guild_channel(guild,
            format!("Team: {}", team_name)
        )
            .kind(ChannelType::GuildCategory);
        match category_request.await {
            Ok(GuildChannel::Category(category)) => {
                // Text Channel
                let text_request = http.create_guild_channel(guild, team_name)
                    .parent_id(category.id)
                    .kind(ChannelType::GuildText);
                match text_request.await {
                    Ok(_) => {
                        // Voice Channel
                        let voice_request = http.create_guild_channel(guild, team_name)
                            .parent_id(category.id)
                            .kind(ChannelType::GuildVoice);
                        match voice_request.await {
                            Ok(_) => {
                                PersistentState::instance().lock().unwrap()
                                    .register_channel_creation(user)
                                    .unwrap_or_else(|e| {
                                        println!("Failed to register channel creation: {:?}", e);
                                    });
                                "Team channels created 🎊"
                            }
                            Err(e) => {
                                println!(
                                    "Failed to create voice channel {}. Error: {:?}",
                                    team_name,
                                    e
                                );
                                "Voice channel creation failed, check logs for details"
                            }
                        }
                    }
                    Err(e) => {
                        println!(
                            "Failed to create text channel {}. Error: {:?}",
                            team_name,
                            e
                        );
                        "Text channel creation failed, check logs for details"
                    }
                }.into()
            }
            Ok(_) => {
                "A channel was created but it wasn't a category 🤔. Blame discord"
                    .into()
            }
            Err(e) => {
                println!(
                    "Failed to create category {}. Error: {:?}",
                    team_name,
                    e
                );
                "Category creation failed, check logs for details".into()
            }
        }
    };

    http.create_message(original_channel)
        .content(&reply)
        .await?;

    Ok(())
}
