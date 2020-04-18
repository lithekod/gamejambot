use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::sync::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::fmt::{Debug, Display};

use tokio::stream::StreamExt;
use serde_derive::{Serialize, Deserialize};
use anyhow::Context;
use lazy_static::lazy_static;
use serde_json;
use regex::{Regex, Captures};
use rand::seq::{SliceRandom, IteratorRandom};

use twilight::{
    cache::{
        twilight_cache_inmemory::config::{InMemoryConfigBuilder, EventType},
        InMemoryCache,
    },
    gateway::cluster::{config::ShardScheme, Cluster, ClusterConfig},
    gateway::shard::Event,
    http::Client as HttpClient,
    http::error::Error as DiscordError,
    model::{
        gateway::GatewayIntents,
        user::{User, CurrentUser},
        channel::{Message, Channel, ChannelType, GuildChannel, Reaction, ReactionType},
        id::{ChannelId, UserId, GuildId, MessageId},
    },
};

type Result<T> = std::result::Result<T, anyhow::Error>;

enum SubmissionResult {
    Done,
    AlreadySubmitted{previous_submission: String},
}

const FILENAME: &'static str = "state.json";
const ORGANIZER: &'static str = "Organizer";
const JAMMER: &'static str = "Jammer";

lazy_static! {
    static ref REQUESTABLE_ROLES : HashSet<String> = {
        let mut set = HashSet::new();
        set.insert("programmer".to_string());
        set.insert("2d artist".to_string());
        set.insert("3d artist".to_string());
        set.insert("sound designer".to_string());
        set.insert("musician".to_string());
        set.insert("idea guy".to_string());
        set.insert("board games".to_string());
        set
    };
}

/**
  Stores state that should persist between bot restarts.

  The data is stored as json and is loaded lazily on the first use
  of the struct.

  Data is not automatically reloaded on file changes
*/
#[derive(Serialize, Deserialize)]
struct PersistentState {
    theme_ideas: HashMap<UserId, String>,
    channel_creators: HashMap<UserId, (String, ChannelId)>,
    eula_channel_id: ChannelId,
    eula_message_id: MessageId,
}

impl PersistentState {
    /// Load the data from disk, or default initialise it if the file doesn't exist
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
                channel_creators: HashMap::new(),
                eula_channel_id: ChannelId {0: 0},
                eula_message_id: MessageId {0: 0},
            })
        }
    }

    /**
      Return a global instance of the struct. The instance is global to
      avoid race conditions, especially with data stored on disk
    */
    pub fn instance() -> &'static Mutex<Self> {
        lazy_static! {
            static ref INSTANCE: Mutex<PersistentState> = Mutex::new(
                PersistentState::load().unwrap()
            );
        }
        &INSTANCE
    }

    /**
      Tries to add a theme submission by the user. Replaces the previous theme
      if the user had one previously. If file saving fails, returns Err
    */
    pub fn try_add_theme(
        &mut self,
        user: UserId,
        idea: &str
    ) -> Result<SubmissionResult> {
        if self.theme_ideas.contains_key(&user) {
            let previous_submission = self.theme_ideas.get(&user).unwrap().to_string();
            self.theme_ideas.insert(user, idea.into());
            self.save().context("Failed to write current themes")?;
            Ok(SubmissionResult::AlreadySubmitted{previous_submission})
        }
        else {
            self.theme_ideas.insert(user, idea.into());
            self.save().context("Failed to write current themes")?;
            Ok(SubmissionResult::Done)
        }
    }

    /// Checks if the user is allowed to create a channel
    pub fn is_allowed_channel(&mut self, id: UserId) -> bool {
        !self.channel_creators.contains_key(&id)
    }

    /// Gets the user's current channel
    pub fn get_channel_info(&mut self, id: UserId) -> Option<&(String, ChannelId)> {
        self.channel_creators.get(&id)
    }

    /// Registers that the user has created a channel
    pub fn register_channel_creation(&mut self, user_id: UserId, game_name: &String, text_id: ChannelId) -> Result<()> {
        self.channel_creators.insert(user_id, (game_name.to_string(), text_id));
        self.save()
    }

    /// Sets the message acting as the server's EULA
    pub fn set_eula(&mut self, channel_id: ChannelId, message_id: MessageId) -> Result<()> {
        self.eula_channel_id = channel_id;
        self.eula_message_id = message_id;
        self.save()
    }

    /// Gets the channel containing the server's EULA
    pub fn get_eula_channel(&mut self) -> ChannelId {
        self.eula_channel_id
    }

    /// Gets the message acting as the server's EULA
    pub fn get_eula_message(&mut self) -> MessageId {
        self.eula_message_id
    }

    /// Save the state to disk. Should be called after all modifications
    pub fn save(&self) -> Result<()> {
        let mut file = File::create(FILENAME)
            .with_context(|| format!("Failed to open {} for writing", FILENAME))?;
        file.write_all(serde_json::to_string(&self)?.as_bytes())
            .with_context(|| format!("Failed to write to {}", FILENAME))?;
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
                | EventType::REACTION_ADD,
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
                handle_reaction_add(&reaction, http)
                    .await?;
            }
        }
        (id, Event::ShardConnected(_)) => {
            println!("Connected on shard {}", id);
        }
        _ => {}
    }

    Ok(())
}


async fn handle_pm(msg: &Message, http: &HttpClient) -> Result<()> {
    // Check if the message is a single word
    if msg.content.split_ascii_whitespace().count() != 1 {
        http.create_message(msg.channel_id)
            .content("Themes ideas should only be a single word.")
            .await?;
    }
    else {
        let had_old_theme = PersistentState::instance().lock()
            .unwrap()
            .try_add_theme(msg.author.id, &msg.content)
            .context("Failed to save theme")?;

        match had_old_theme {
            SubmissionResult::Done => {
                // Check if the message is a PM
                http.create_message(msg.channel_id)
                    .content(format!(
                        "Theme idea \"{}\" registered, thanks!",
                        &msg.content
                    ))
                    .await?;
            }
            SubmissionResult::AlreadySubmitted{previous_submission} => {
                // Check if the message is a PM
                http.create_message(msg.channel_id)
                    .content(format!(
                        "You can only submit one idea.\n\
                        Theme idea \"{}\" registered, \
                        replacing you previous submission \"{}\".",
                        &msg.content, previous_submission
                    ))
                    .await?;
            }
        }
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
        Some("!help") => {
            send_help_message(
                http,
                msg.channel_id,
                msg.author.id,
                msg.guild_id.expect("Tried to call for help in non-guild"),
            ).await?;
        }
        Some("!createchannels") => {
            let guild_id = msg.guild_id.expect("Tried to create channel in non-guild");

            // To prevent use before the jam
            if !has_role(&http, guild_id, msg.author.id, JAMMER).await?
            && !has_role(&http, guild_id, msg.author.id, ORGANIZER).await? {
                send_message(&http, msg.channel_id, msg.author.id,
                    format!(
                        "Oo, you found a secret command. 😉\n\
                        You will be able to use this command once you have \
                        been assigned the **{}** role.\n\
                        You will be able to get this role once the jam has \
                        started. The details on how to do so will be made \
                        available at that point.",
                        JAMMER
                    )
                ).await?;
                return Ok(())
            }

            let result = handle_create_team_channels(
                &words.collect::<Vec<_>>(),
                guild_id,
                msg.author.id,
                &http
            ).await;

            match result {
                Ok(team) => {
                    send_message(&http, msg.channel_id, msg.author.id,
                        format!(
                            "Channels created for your game {} here: <#{}>",
                            team.game_name, team.text_id
                        )
                    ).await?;
                }
                Err(ref e) => {
                    send_message(&http, msg.channel_id, msg.author.id,
                        format!("{}", e)
                    ).await?;
                    println!("Channel creation failed: {:?}", e);
                }
            }
        },
        Some("!role") => {
            handle_give_role(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to create channel in non-guild"),
                &msg.author,
                http
            ).await?;
        },
        Some("!leave") => {
            handle_remove_role(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to create channel in non-guild"),
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
        Some("!seteula") => {
            handle_set_eula(
                &words.collect::<Vec<_>>(),
                msg.channel_id,
                msg.guild_id.expect("Tried to set EULA in non-guild"),
                &msg.author,
                http,
                msg,
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

async fn handle_reaction_add(
    reaction: &Reaction,
    http: HttpClient,
) -> Result<()> {
    let mut ps = PersistentState::instance().lock().unwrap();
    if reaction.channel_id == ps.get_eula_channel() &&
        reaction.message_id == ps.get_eula_message() {

        match &reaction.emoji {
            ReactionType::Unicode {name} => {
                if name == "👍" {
                    let reactor = &reaction.member.as_ref().unwrap().user;
                    let guild = reaction.guild_id.unwrap();
                    let guild_roles = http.roles(guild).await?;
                    for role in guild_roles {
                        if role.name.to_lowercase() == JAMMER.to_lowercase() {
                            let request = http.add_guild_member_role(guild, reactor.id, role.id);

                            match request.await {
                                Ok(_) => {
                                    println!("EULA accepted: New role {} assigned to {}", role.name, reactor.name);
                                }
                                Err(e) => {
                                    println!("EULA accepted: Couldn't assign role {} to {}\n{}", role.name, reactor.name, e);
                                }
                            }
                            return Ok(())
                        }
                    }
                    println!("No role {} specified on the server", JAMMER);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn send_message(
    http: &HttpClient,
    channel_id: ChannelId,
    user_id: UserId,
    content: impl Into<String> + Display,
) -> Result<()> {
    http.create_message(channel_id)
        .content(format!("<@{}> {}", user_id, content))
        .await?;
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
        with the command `!createchannels <game name>`.";
    let organizer_message = format!(
        "Since you have the **{}** role, you also have access to the \
        following commands:\n\
        - `!generatetheme` to generate a theme.\n\
        - `!seteula <mention of channel with the message> <message ID>` to \
        set the message acting as the server's EULA.", ORGANIZER
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

async fn has_role(
    http: &HttpClient,
    guild_id: GuildId,
    user_id: UserId,
    role: impl ToString,
) -> Result<bool> {
    let guild_roles = http.roles(guild_id).await?;
    let user_roles = http.guild_member(guild_id, user_id).await?.unwrap().roles;
    let role_to_check = role.to_string().to_lowercase();

    for role in guild_roles {
        if role.name.to_lowercase() == role_to_check
            && user_roles.contains(&role.id)
        {
            return Ok(true)
        }
    }
    Ok(false)
}

fn do_theme_generation() -> String {
    let mut rng = rand::thread_rng();
    let ref theme_ideas = PersistentState::instance().lock().unwrap().theme_ideas;
    let mut selected = theme_ideas
        .iter()
        .map(|(_, idea)| idea)
        .choose_multiple(&mut rng, 2);

    // Per documetation: The order of chose_multiple is not random. To achieve
    // that, shuffle the result
    selected.shuffle(&mut rng);

    if selected.len() != 2 {
        "Not enough ideas have been submitted yet.".to_string()
    }
    else {
        format!("The theme is: {} {}", selected[0], selected[1])
    }
}



async fn handle_create_team_channels<'a>(
    rest_command: &[&'a str],
    guild: GuildId,
    user: UserId,
    http: &HttpClient
) -> std::result::Result<CreatedTeam, ChannelCreationError<>> {
    lazy_static! {
        static ref INVALID_REGEX: Regex = Regex::new("[`]+").unwrap();
        static ref MARKDOWN_ESCAPE_REGEX: Regex = Regex::new("[-_+*\"#=.⋅\\\\<>{}]+").unwrap();
    }

    if !PersistentState::instance().lock().unwrap().is_allowed_channel(user) {
        Err(ChannelCreationError::AlreadyCreated(user))
    }
    else {
        let game_name = &*rest_command.join(" ");
        println!("Got a request for channels for the game {:?}", game_name);
        if rest_command.len() == 0 {
            Err(ChannelCreationError::NoName)
        }
        else if INVALID_REGEX.is_match(game_name) {
            Err(ChannelCreationError::InvalidName)
        }
        else {
            let category_name = format!("Team: {}", game_name);
            // Create a category
            let category = http.create_guild_channel(guild, category_name)
                .kind(ChannelType::GuildCategory)
                .await
                .map_err(ChannelCreationError::CategoryCreationFailed)
                .and_then(|maybe_category| {
                    match maybe_category {
                        GuildChannel::Category(category) => {
                            Ok(category)
                        }
                        _ => Err(ChannelCreationError::CategoryNotCreated)
                    }
                })?;

            let text = http.create_guild_channel(guild, game_name)
                .parent_id(category.id)
                .kind(ChannelType::GuildText)
                .topic(format!("Work on and playtesting of the game {}.", game_name))
                .await
                .map_err(|e| ChannelCreationError::TextCreationFailed(e))
                .and_then(|maybe_text| {
                    match maybe_text {
                        GuildChannel::Category(text) => { // For some reason it isn't a GuildChannel::Text
                            Ok(text)
                        }
                        _ => Err(ChannelCreationError::TextNotCreated)
                    }
                })?;

            http.create_guild_channel(guild, game_name)
                .parent_id(category.id)
                .kind(ChannelType::GuildVoice)
                .await
                .map_err(|e| ChannelCreationError::VoiceCreationFailed(e))?;

            let game_name_markdown_safe = MARKDOWN_ESCAPE_REGEX.replace_all(game_name,
                |caps: &Captures| {
                    format!("\\{}", &caps[0])
                }
            ).to_string();
            println!("Markdown-safe name: {}", game_name_markdown_safe);

            PersistentState::instance().lock().unwrap()
                .register_channel_creation(user, &game_name_markdown_safe, text.id)
                .unwrap();

            Ok(CreatedTeam{
                game_name: game_name_markdown_safe,
                text_id: text.id
            })
        }
    }
}

/**
  Info about the channels created for a team
*/
#[derive(Debug)]
struct CreatedTeam {
    pub game_name: String,
    pub text_id: ChannelId
}

/**
  Error type for channel creation attempts

  The Display implementation is intended to be sent back to the user
*/
#[derive(Debug)]
enum ChannelCreationError {
    /// The user has already created a channel
    AlreadyCreated(UserId),
    /// No name was specified
    NoName,
    /// The user used invalid characters in the channel name
    InvalidName,
    /// The discord API said everything was fine but created something
    /// that was not a category
    CategoryNotCreated,
    /// The discord API said everything was fine but created something
    /// that was not a text channel
    TextNotCreated,
    /// The discord API said everything was fine but created something
    /// that was not a voice channel
    VoiceNotCreated,
    /// The discord API returned an error when creating category
    CategoryCreationFailed(DiscordError),
    /// The discord API returned an error when creating text channel
    TextCreationFailed(DiscordError),
    /// The discord API returned an error when creating voice channel
    VoiceCreationFailed(DiscordError)
}

async fn handle_give_role<'a>(
    rest_command: &[&'a str],
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient
) -> Result<()> {
    let mut message = "You need to to specify a valid role.\nAvailable roles are:```\nProgrammer\n2D Artist\n3D Artist\nSound Designer\nMusician\nIdea Guy\nBoard Games```".to_string();

    let reply : String = if rest_command.len() == 0 {
        message.into()
    }
    else {
        let requested_role = rest_command.join(" ").to_lowercase();
        if REQUESTABLE_ROLES.contains(&requested_role) {
            let guild_roles = http.roles(guild).await?;
            let author_roles = http.guild_member(guild, author.id).await?.unwrap().roles;

            for role in guild_roles {
                if role.name.to_lowercase() == requested_role {
                    if !author_roles.contains(&role.id) {
                        let request = http.add_guild_member_role(guild, author.id, role.id);

                        match request.await {
                            Ok(_) => {
                                message = format!("You have been assigned the role **{}**.", role.name);
                                println!("New role {} assigned to {}", role.name, author.name);
                            }
                            Err(e) => {
                                println!("Couldn't assign role {} to {}\n{}", role.name, author.name, e);
                            }
                        }
                    }
                    else {
                        message = format!("You already have the role **{}**.", role.name);
                        println!("{} already has the role ({}) they are trying to get", author.name, role.name);
                    }
                }
            }
        }
        message.into()
    };

    send_message(&http, original_channel, author.id, reply).await?;

    Ok(())
}

async fn handle_remove_role<'a>(
    rest_command: &[&'a str],
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient
) -> Result<()> {
    let mut message = "You need to to specify a valid role.\nAvailable roles are:```\nProgrammer\n2D Artist\n3D Artist\nSound Designer\nMusician\nIdea Guy\nBoard Games```".to_string();

    let reply : String = if rest_command.len() == 0 {
        message.into()
    }
    else {
        let requested_role = rest_command.join(" ").to_lowercase();
        if REQUESTABLE_ROLES.contains(&requested_role) {
            let guild_roles = http.roles(guild).await?;
            let author_roles = http.guild_member(guild, author.id).await?.unwrap().roles;

            for role in guild_roles {
                if role.name.to_lowercase() == requested_role {
                    if author_roles.contains(&role.id) {
                        let request = http.remove_guild_member_role(guild, author.id, role.id);

                        match request.await {
                            Ok(_) => {
                                message = format!("You have been stripped of the role **{}**.", role.name);
                                println!("{} left the role {}", author.name, role.name);
                            }
                            Err(e) => {
                                println!("Couldn't remove role {} from {}\n{}", role.name, author.name, e);
                            }
                        }
                    }
                    else {
                        message = format!("You don't have the role **{}**.", role.name);
                        println!("{} tried to leave a role ({}) they didn't have", author.name, role.name);
                    }
                }
            }
        }
        message.into()
    };

    send_message(&http, original_channel, author.id, reply).await?;

    Ok(())
}

async fn handle_generate_theme(
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient
) -> Result<()> {
    if has_role(
        &http,
        guild,
        author.id,
        ORGANIZER,
    ).await? {
        let theme = do_theme_generation();
        let send_result = send_message(&http, original_channel, author.id,
            &theme
        )
        .await
        .context("Failed to send theme");
        match send_result {
            Ok(_) => {},
            Err(e) => {
                send_message(&http, original_channel, author.id,
                    "Failed to send theme. Has someone been naughty? 🤔"
                ).await?;
                println!("Failed to send theme message {:?}", e);
                println!("Message should have been: {:?}", theme);
            }
        }
    }
    else {
        send_message(&http, original_channel, author.id,
            format!(
                "Since you lack the required role **{}**, you do \
                not have permission to generate themes.", ORGANIZER)
        ).await?;
        println!("Tried to generate theme without required role \"{}\"", ORGANIZER);
    }

    Ok(())
}

async fn handle_set_eula<'a>(
    rest_command: &[&'a str],
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient,
    msg: &Message,
) -> Result<()> {
    lazy_static! {
        static ref CHANNEL_MENTION_REGEX: Regex =
            Regex::new(r"<#(\d+)>").unwrap();
    }
    println!("Got set EULA request \"{}\"", &msg.content);

    if has_role(
        &http,
        guild,
        author.id,
        ORGANIZER,
    ).await? {

        // Parse arguments
        let arg_guide_msg = "Proper usage: `!seteula <mention of channel with the message> <message ID>`";
        if rest_command.len() < 2 {
            send_message(&http, original_channel, author.id, arg_guide_msg).await?;
        }
        else {
            match CHANNEL_MENTION_REGEX.captures(rest_command[0]) {
                Some(channel_ids) => {
                    if channel_ids.len() != 2 {
                        send_message(&http, original_channel, author.id,
                            format!("Invalid channel reference.\n{}", arg_guide_msg)
                        ).await?;
                    }
                    else {
                        match channel_ids[1].parse::<u64>() {
                            Ok(channel_id_num) => {
                                match rest_command[1].parse::<u64>() {
                                    Ok(messege_id_num) => {

                                        // Fetch EULA message
                                        match http.message(
                                            ChannelId {0: channel_id_num},
                                            MessageId {0: messege_id_num}
                                        ).await {
                                            Ok(response) => {
                                                let eula_msg = response.unwrap();
                                                let mut ps = PersistentState::instance().lock().unwrap();
                                                let result = ps.set_eula(eula_msg.channel_id, eula_msg.id);

                                                match result {
                                                    Ok(_) => {
                                                        send_message(&http, original_channel, author.id,
                                                            format!(
                                                                "Server EULA set to the following messege by <@{}> in <#{}>:\n>>> {}",
                                                                eula_msg.author.id, eula_msg.channel_id, eula_msg.content
                                                            )
                                                        ).await?;
                                                    }
                                                    Err(ref e) => {
                                                        send_message(&http, original_channel, author.id,
                                                            format!("Could not set server EULA. Check the logs for details.")
                                                        ).await?;
                                                        println!("EULA setting failed: {:?}", e);
                                                    }
                                                }
                                            }
                                            Err(_) => {
                                                send_message(&http, original_channel, author.id,
                                                    format!(
                                                        "No message with ID {} was found in <#{}>",
                                                        messege_id_num, channel_id_num
                                                    )
                                                ).await?;
                                                println!("No message with ID {} was found in <#{}>",
                                                    messege_id_num, channel_id_num
                                                );
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        send_message(&http, original_channel, author.id,
                                            format!("Message ID must be a number.\n{}", arg_guide_msg)
                                        ).await?;
                                    }
                                }
                            }
                            Err(_) => {
                                send_message(&http, original_channel, author.id,
                                    format!("Invalid channel reference.\n{}", arg_guide_msg)
                                ).await?;
                            }
                        }
                    }
                }
                _ => {
                    send_message(&http, original_channel, author.id,
                        format!("Invalid channel reference.\n{}", arg_guide_msg)
                    ).await?;
                }
            }
        }
    }
    else {
        send_message(&http, original_channel, author.id,
            format!(
                "Since you lack the required role **{}**, you do \
                not have permission to set the server EULA.", ORGANIZER)
        ).await?;
        println!("Tried to set EULA without required role \"{}\"", ORGANIZER);
    }

    Ok(())
}

impl Display for ChannelCreationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::AlreadyCreated(user) => {
                let mut ps = PersistentState::instance().lock().unwrap();
                let (game_name, text_id) = ps.get_channel_info(*user).unwrap();
                format!("You have already created channels for your game {} here: <#{}>",
                    game_name, text_id)
            }
            Self::NoName => "You need to specify a game name.".to_string(),
            Self::CategoryNotCreated =>
                "I asked Discord for a category but got something else. 🤔".to_string(),
            Self::TextNotCreated =>
                "I asked Discord for a text channel but got something else. 🤔".to_string(),
            Self::VoiceNotCreated =>
                "I asked Discord for a voice channel but got something else. 🤔".to_string(),
            Self::InvalidName =>
                "Game names cannot contain the character `".to_string(),
            Self::CategoryCreationFailed(_) => "Category creation failed.".to_string(),
            Self::TextCreationFailed(_) => "Text channel creation failed.".to_string(),
            Self::VoiceCreationFailed(_) => "Voice channel creation failed.".to_string(),
        };
        write!(f, "{}", msg)
    }
}

impl std::error::Error for ChannelCreationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlreadyCreated(_)
                | Self::NoName
                | Self::CategoryNotCreated
                | Self::TextNotCreated
                | Self::VoiceNotCreated
                | Self::InvalidName => None,
            Self::CategoryCreationFailed(e)
                | Self::TextCreationFailed(e)
                | Self::VoiceCreationFailed(e) => Some(e)
        }
    }
}
