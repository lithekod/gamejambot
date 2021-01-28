use std::collections::HashSet;
use std::fmt::Display;

use lazy_static::lazy_static;
use twilight::{
    http::Client as HttpClient,
    http::error::Error as DiscordError,
    model::{
        id::{ChannelId, UserId, GuildId},
        user::User,
    },
};

use crate::roles::*;
use crate::utils::{Result, send_message};


lazy_static! {
    static ref REQUESTABLE_ROLES : HashSet<String> = {
        let mut set = HashSet::new();
        set.insert(PROGRAMMER.to_lowercase());
        set.insert(ARTIST_2D.to_lowercase());
        set.insert(ARTIST_3D.to_lowercase());
        set.insert(SOUND_DESIGNER.to_lowercase());
        set.insert(MUSICIAN.to_lowercase());
        set.insert(IDEA_GUY.to_lowercase());
        set.insert(BOARD_GAMES.to_lowercase());
        set.insert(PLAY_TESTER.to_lowercase());
        set
    };
}

pub async fn has_role(
    http: &HttpClient,
    guild_id: GuildId,
    user_id: UserId,
    role_name: impl ToString,
) -> Result<bool> {
    let guild_roles = http.roles(guild_id).await?;
    let user_roles = http.guild_member(guild_id, user_id).await?.unwrap().roles;
    let role_to_check = role_name.to_string().to_lowercase();

    for role in guild_roles {
        if role.name.to_lowercase() == role_to_check
            && user_roles.contains(&role.id)
        {
            return Ok(true)
        }
    }
    Ok(false)
}

impl std::error::Error for RoleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidName(_)
                | Self::AlreadySet(_)
                | Self::NotSet(_) => None,
            Self::RequestFailed(e) => Some(e)
        }
    }
}

pub async fn set_role(
    http: &HttpClient,
    guild: GuildId,
    user_id: UserId,
    role_name: impl ToString,
) -> std::result::Result<String, RoleError<>> {
    let requested_role = role_name.to_string().to_lowercase();
    let guild_roles = http.roles(guild).await?;
    let author_roles = http.guild_member(guild, user_id).await?.unwrap().roles;

    for role in guild_roles {
        if role.name.to_lowercase() == requested_role {
            return if !author_roles.contains(&role.id) {
                let request = http.add_guild_member_role(guild, user_id, role.id);

                match request.await {
                    Err(e) => {
                        Err(RoleError::RequestFailed(e))
                    }
                    _ => {
                        Ok(role.name)
                    }
                }
            }
            else {
                Err(RoleError::AlreadySet(role.name))
            }
        }
    }
    Err(RoleError::InvalidName(role_name.to_string()))
}

pub async fn remove_role(
    http: &HttpClient,
    guild: GuildId,
    user_id: UserId,
    role_name: impl ToString,
) -> std::result::Result<String, RoleError<>> {
    let requested_role = role_name.to_string().to_lowercase();
    let guild_roles = http.roles(guild).await?;
    let author_roles = http.guild_member(guild, user_id).await?.unwrap().roles;

    for role in guild_roles {
        if role.name.to_lowercase() == requested_role {
            return if author_roles.contains(&role.id) {
                let request = http.remove_guild_member_role(guild, user_id, role.id);

                match request.await {
                    Err(e) => {
                        Err(RoleError::RequestFailed(e))
                    }
                    _ => {
                        Ok(role.name)
                    }
                }
            }
            else {
                Err(RoleError::NotSet(role.name))
            }
        }
    }
    Err(RoleError::InvalidName(role_name.to_string()))
}

pub async fn handle_give_role<'a>(
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
        let requested_role = rest_command.join(" ");
        if REQUESTABLE_ROLES.contains(&requested_role.to_lowercase()) {
            match set_role(&http, guild, author.id, &requested_role).await {
                Err(e) => {
                    message = format!("Couldn't assign role to you: {}", e);
                    println!("Couldn't assign role to {}: {}", author.name, e);
                }
                Ok(role) => {
                    message = format!("You have been assigned the role **{}**.", role);
                    println!("New role {} assigned to {}", role, author.name);
                }
            }
        }
        message.into()
    };

    send_message(&http, original_channel, author.id, reply).await?;

    Ok(())
}

pub async fn handle_remove_role<'a>(
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
        let requested_role = rest_command.join(" ");
        if REQUESTABLE_ROLES.contains(&requested_role.to_lowercase()) {
            match remove_role(&http, guild, author.id, &requested_role).await {
                Err(e) => {
                    message = format!("Couldn't strip you of role: {}", e);
                    println!("Couldn't strip {} of role: {}", author.name, e);
                }
                Ok(role) => {
                    message = format!("You have been stripped of the role **{}**.", role);
                    println!("{} left the role {}", author.name, role);
                }
            }
        }
        message.into()
    };

    send_message(&http, original_channel, author.id, reply).await?;

    Ok(())
}

#[derive(Debug)]
pub enum RoleError {
    RequestFailed(DiscordError),
    InvalidName(String),
    AlreadySet(String),
    NotSet(String),
}

impl From<DiscordError> for RoleError {
    fn from(e: DiscordError) -> Self {
        Self::RequestFailed(e)
    }
}

impl Display for RoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::RequestFailed(e) =>
                format!("Discord error: {}", e),
            Self::InvalidName(invalid_name) =>
                format!("Invalid role name \"{}\"", invalid_name),
            Self::AlreadySet(role) =>
                format!("Role **{}** already set", role),
            Self::NotSet(role) =>
                format!("Role **{}** not set", role),
        };
        write!(f, "{}", msg)
    }
}
