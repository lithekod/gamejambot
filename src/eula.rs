
use regex::Regex;
use lazy_static::lazy_static;
use twilight::{
    http::Client as HttpClient,
    model::{
        channel::{Message, Reaction, ReactionType},
        id::{ChannelId, GuildId, MessageId},
        user::User,
    },
};

use crate::role::{JAMMER, ORGANIZER, has_role};
use crate::state::PersistentState;
use crate::utils::{Result, send_message};


pub async fn handle_accept_eula(
    http: HttpClient,
    reaction: &Reaction,
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

pub async fn handle_set_eula<'a>(
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
