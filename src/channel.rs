use std::clone::Clone;
use std::vec::Vec;
use std::fmt::Display;

use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serde_derive::{Serialize, Deserialize};
use twilight::{
    http::Client as HttpClient,
    http::error::Error as DiscordError,
    model::{
        channel::{Channel, ChannelType, GuildChannel},
        id::{ChannelId, GuildId, UserId},
    },
};

use crate::role::{JAMMER, ORGANIZER, has_role};
use crate::state::PersistentState;
use crate::utils::{Result, send_message};

#[derive(Serialize, Deserialize, Clone)]
pub struct Team {
    game_name: String,
    category_id: ChannelId,
    text_id: ChannelId,
    voice_id: ChannelId,
}

pub async fn handle_create_channels<'a>(
    rest_command: &[&'a str],
    original_channel_id: ChannelId,
    guild_id: GuildId,
    user_id: UserId,
    http: HttpClient
) -> Result<()> {

    // To prevent use before the jam
    if !has_role(&http, guild_id, user_id, JAMMER).await?
    && !has_role(&http, guild_id, user_id, ORGANIZER).await? {
        send_message(&http, original_channel_id, user_id,
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

    let result = create_team(
        rest_command,
        guild_id,
        user_id,
        &http
    ).await;

    match result {
        Ok(team) => {
            send_message(&http, original_channel_id, user_id,
                format!(
                    "Channels created for your game **{}** here: <#{}>",
                    team.game_name, team.text_id
                )
            ).await?;
        }
        Err(ref e) => {
            send_message(&http, original_channel_id, user_id,
                format!("{}", e)
            ).await?;
            println!("Channel creation failed: {:?}", e);
        }
    }
    Ok(())
}

pub async fn handle_remove_channels<'a>(
    rest_command: &[&'a str],
    original_channel_id: ChannelId,
    guild_id: GuildId,
    author_id: UserId,
    http: HttpClient
) -> Result<()> {
    // Only let organizers use this command
    if !has_role(&http, guild_id, author_id, ORGANIZER).await? {
        send_message(&http, original_channel_id, author_id,
            format!("WAT")
        ).await?
    }
    else {
        if rest_command.len() > 0 {

            let id = match rest_command.join("").parse::<u64>() {
                Ok(id) => id,
                Err(_) => {
                    send_message(&http, original_channel_id, author_id,
                        format!("That user id is invalid.")
                    ).await?;
                    return Ok(())
                },
            };

            let user_id = UserId(id);

            if PersistentState::instance().lock().unwrap().is_allowed_channel(user_id) {
                send_message(&http, original_channel_id, author_id,
                    format!("That user does not have any team channels.")
                ).await?;
            }
            else {
                let team = PersistentState::instance().lock().unwrap().get_channel_info(user_id).cloned().unwrap();

                let mut oks = Vec::new();
                let mut errs = Vec::new();
                match http.delete_channel(team.category_id).await {
                    Ok(Channel::Guild(GuildChannel::Category(category))) => {
                        oks.push(format!("category **{}**", category.name));
                    }
                    _ => {
                        errs.push("category".to_string());
                    }
                }
                match http.delete_channel(team.text_id).await {
                    Ok(Channel::Guild(GuildChannel::Category(text))) => {
                        oks.push(format!("text channel **#{}**", text.name));
                    }
                    _ => {
                        errs.push("text channel".to_string());
                    }
                }
                match http.delete_channel(team.voice_id).await {
                    Ok(Channel::Guild(GuildChannel::Category(voice))) => {
                        oks.push(format!("voice channel **{}**", voice.name));
                    }
                    _ => {
                        errs.push("voice channel".to_string());
                    }
                }

                PersistentState::instance().lock().unwrap().remove_channel(user_id).unwrap();

                let message =
                if oks.len() > 0 {
                    if errs.len() > 0 {
                        let have_has = if errs.len() > 1 { "have" } else { "has" };
                        format!("Removed {} for the game **{}** but its {} {} already been removed.",
                            list_strings(oks), team.game_name, list_strings(errs), have_has
                        )
                    }
                    else {
                        format!("Removed {} for the game **{}**.",
                            list_strings(oks), team.game_name
                        )
                    }
                }
                else {
                    format!("Category, text channel and voice channel for the game **{}** have already been removed.",
                        team.game_name
                    )
                };

                send_message(&http, original_channel_id, author_id, message).await?;
            }
        }
        else {
            send_message(&http, original_channel_id, author_id,
                "You forgot to provide a user id."
            ).await?;
            return Ok(())
        }
    }
    Ok(())
}

fn list_strings(
    strings: Vec<String>
) -> String {
    let mut result = "".to_string();
    for i in 0..strings.len() {
        if i > 0 {
            if i == strings.len() - 1 {
                result.push_str(" and ");
            }
            else {
                result.push_str(", ");
            }
        }
        result.push_str(&strings[i]);
    }
    result
}

async fn create_team<'a>(
    rest_command: &[&'a str],
    guild: GuildId,
    user: UserId,
    http: &HttpClient
) -> std::result::Result<Team, ChannelCreationError<>> {
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

            let voice = http.create_guild_channel(guild, game_name)
                .parent_id(category.id)
                .kind(ChannelType::GuildVoice)
                .await
                .map_err(|e| ChannelCreationError::VoiceCreationFailed(e))
                .and_then(|maybe_voice| {
                    match maybe_voice {
                        GuildChannel::Category(voice) => { // For some reason it isn't a GuildChannel::Voice
                            Ok(voice)
                        }
                        _ => Err(ChannelCreationError::VoiceNotCreated)
                    }
                })?;

            let game_name_markdown_safe = MARKDOWN_ESCAPE_REGEX.replace_all(game_name,
                |caps: &Captures| {
                    format!("\\{}", &caps[0])
                }
            ).to_string();
            println!("Markdown-safe name: {}", game_name_markdown_safe);

            let team = Team {
                game_name: game_name_markdown_safe,
                category_id: category.id,
                text_id: text.id,
                voice_id: voice.id
            };
            PersistentState::instance().lock().unwrap()
                .register_channel_creation(user, &team)
                .unwrap();

            Ok(team)
        }
    }
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

impl Display for ChannelCreationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::AlreadyCreated(user) => {
                let mut ps = PersistentState::instance().lock().unwrap();
                let team = ps.get_channel_info(*user).unwrap();
                format!("You have already created channels for your game **{}** here: <#{}>",
                    team.game_name, team.text_id)
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
