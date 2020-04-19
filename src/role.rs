use std::collections::HashSet;

use lazy_static::lazy_static;
use twilight::{
    http::Client as HttpClient,
    model::{
        id::{ChannelId, UserId, GuildId},
        user::User,
    },
};

use crate::utils::{Result, send_message};

pub const ORGANIZER: &'static str = "Organizer";
pub const JAMMER: &'static str = "Jammer";

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

pub async fn has_role(
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
