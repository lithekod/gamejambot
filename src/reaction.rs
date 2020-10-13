use regex::Regex;
use lazy_static::lazy_static;
use twilight::{
    http::Client as HttpClient,
    model::{
        channel::{Message, Reaction, ReactionType},
        id::{ChannelId, GuildId, MessageId},
        user::{User, CurrentUser},
    },
};

use crate::role::{has_role, remove_role, set_role};
use crate::roles::*;
use crate::state::PersistentState;
use crate::utils::{Result, send_message};


pub async fn handle_reaction_add(
    reaction: &Reaction,
    http: HttpClient,
    current_user: &CurrentUser,
) -> Result<()> {
    handle_add_role(&http, reaction, &current_user).await?;
    Ok(())
}

pub async fn handle_reaction_remove(
    reaction: &Reaction,
    http: HttpClient,
) -> Result<()> {
    handle_remove_role(&http, reaction).await?;
    Ok(())
}

fn emoji_to_role(emoji: &String) -> Option<&str> {
    if      emoji == "💻" { Some(PROGRAMMER) }
    else if emoji == "🎨" { Some(ARTIST_2D) }
    else if emoji == "🗿" { Some(ARTIST_3D) }
    else if emoji == "🔊" { Some(SOUND_DESIGNER) }
    else if emoji == "🎵" { Some(MUSICIAN) }
    else if emoji == "💡" { Some(IDEA_GUY) }
    else if emoji == "🎲" { Some(BOARD_GAMES) }
    else { None }
}

async fn handle_add_role(
    http: &HttpClient,
    reaction: &Reaction,
    current_user: &CurrentUser,
) -> Result<()> {
    let mut ps = PersistentState::instance().lock().unwrap();
    if reaction.channel_id == ps.get_role_assign_channel() &&
        reaction.message_id == ps.get_role_assign_message() {

        let guild_id = reaction.guild_id.unwrap();
        let user_id = reaction.user_id;

        if user_id != current_user.id {
            match &reaction.emoji {
                ReactionType::Unicode {name} => {
                    let maybe_role = emoji_to_role(name);
                    match maybe_role {
                        Some(role_name) => {
                            match set_role(http, guild_id, user_id, role_name).await {
                                Err(e) => println!("Failed setting role from reaction {}: {}", name, e),
                                _ => {}
                            }
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
        else {}
    }
    Ok(())
}

async fn handle_remove_role(
    http: &HttpClient,
    reaction: &Reaction,
) -> Result<()> {
    let mut ps = PersistentState::instance().lock().unwrap();
    if reaction.channel_id == ps.get_role_assign_channel() &&
        reaction.message_id == ps.get_role_assign_message() {

        let guild_id = reaction.guild_id.unwrap();
        let user_id = reaction.user_id;

        match &reaction.emoji {
            ReactionType::Unicode {name} => {
                let maybe_role = emoji_to_role(name);
                match maybe_role {
                    Some(role_name) => {
                        match remove_role(http, guild_id, user_id, role_name).await {
                            Err(e) => println!("Failed to remove role from reaction {}: {}", name, e),
                            _ => {}
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub enum ReactionMessageType {
    RoleAssign,
}

pub async fn handle_set_reaction_message<'a>(
    rest_command: &[&'a str],
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient,
    msg: &Message,
    msg_type: ReactionMessageType,
) -> Result<()> {
    lazy_static! {
        static ref CHANNEL_MENTION_REGEX: Regex =
            Regex::new(r"<#(\d+)>").unwrap();
    }
    let msg_type_name = match msg_type {
        ReactionMessageType::RoleAssign => "role assignment message",
    };

    println!("Got set {} request \"{}\"", msg_type_name, &msg.content);

    if has_role(
        &http,
        guild,
        author.id,
        ORGANIZER,
    ).await? {

        // Parse arguments
        let command = match msg_type {
            ReactionMessageType::RoleAssign => "setroleassign",
        };
        let arg_guide_msg = format!(
            "Proper usage: `!{} <mention of channel with the message> <message ID>`", command
        );
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

                                        // Fetch specified message
                                        match http.message(
                                            ChannelId(channel_id_num),
                                            MessageId(messege_id_num)
                                        ).await {
                                            Ok(response) => {
                                                let reaction_msg = response.unwrap();
                                                let mut ps = PersistentState::instance().lock().unwrap();
                                                let result = match msg_type {
                                                    ReactionMessageType::RoleAssign => {
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "💻").await?;
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "🎨").await?;
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "🗿").await?;
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "🔊").await?;
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "🎵").await?;
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "💡").await?;
                                                        http.create_reaction(reaction_msg.channel_id, reaction_msg.id, "🎲").await?;
                                                        ps.set_role_assign(reaction_msg.channel_id, reaction_msg.id)
                                                    }
                                                };

                                                match result {
                                                    Ok(_) => {
                                                        send_message(&http, original_channel, author.id,
                                                            format!(
                                                                "Server {} set to the following messege by <@{}> in <#{}>:\n>>> {}",
                                                                msg_type_name, reaction_msg.author.id,
                                                                reaction_msg.channel_id, reaction_msg.content
                                                            )
                                                        ).await?;
                                                    }
                                                    Err(ref e) => {
                                                        send_message(&http, original_channel, author.id,
                                                            format!("Could not set server {}. Check the logs for details.", msg_type_name)
                                                        ).await?;
                                                        println!("Failed setting {}: {:?}", msg_type_name, e);
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
                not have permission to set the server {}.", ORGANIZER, msg_type_name)
        ).await?;
        println!("Tried to set {} without required role \"{}\"", msg_type_name, ORGANIZER);
    }

    Ok(())
}
