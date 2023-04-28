use std::error::Error;

use futures_util::{StreamExt, SinkExt};
use ggstdl::{Move, GGSTDLData};
use regex::Regex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

const TWITCH_IRC_ADDRESS: &str = "ws://irc-ws.chat.twitch.tv:80";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    let pass = std::env::var("TWITCH_TOKEN")?;
    let nick = "dustloopbot".to_string();
    let channels = ["sagan37", "BedlessSleeper", "fgcsand", "me_lolo"].map(|s| format!("#{}", s)).join(",");

    let url = url::Url::parse(TWITCH_IRC_ADDRESS)?;

    let mut val = web_socket_loop(&url, &pass, &nick, &channels).await;
    while let Err(_) = val {
        println!("Connection closed, resetting...");
        val = web_socket_loop(&url, &pass, &nick, &channels).await;
    }

    Ok(())
}

async fn web_socket_loop(url: &Url, pass: &String, nick: &String, channels: &String) -> Result<(), Box<dyn Error>> {
    let (mut ws_stream, _) = connect_async(url).await?;
    
    ws_stream.send(Message::Text(format!("PASS {}", pass))).await?;
    ws_stream.send(Message::Text(format!("NICK {}", nick))).await?;
    ws_stream.send(Message::Text(format!("JOIN {}", channels))).await?;

    let data = ggstdl::load().await.expect("Could not load DustloopInfo");

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if let Ok(text) = msg.to_text() {
            println!("{}", text);

            if text.starts_with("PING") {
                let msg = text.splitn(2, " ").skip(1).next().unwrap();
                ws_stream.send(Message::Text(format!("PONG {}", msg))).await?;
                continue;
            }

            if let Some(command) = parse_message_to_command(text) {
                println!("{:?}", command);
                if command.command.eq_ignore_ascii_case("!fd") {
                    match parse_frames_command(command.args, &data) {
                        Ok(move_found) => {
                            let move_print = format_move(move_found);
                            ws_stream.send(format_msg(move_print, command.channel)).await?
                        },
                        Err(err) => {
                            match err {
                                ParseFramesCommandError::UnknownCharacter(query) => {
                                    ws_stream.send(format_msg(format!("Currently unknown character: '{}'", query), command.channel)).await?;
                                },
                                ParseFramesCommandError::UnknownMove(query) => {
                                    ws_stream.send(format_msg(format!("Currently unknown move: '{}'", query), command.channel)).await?;
                                },
                                ParseFramesCommandError::WrongArguments => {
                                    ws_stream.send(format_msg("Invalid args, try: !frames <char> <move_query>".to_string(), command.channel)).await?;
                                },
                            }
                        }
                    }
                }
            }
        }
    }
    ws_stream.close(None).await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct Command {
    pub channel: String,
    pub command: String,
    pub args: Vec<String>
}

#[derive(Debug, Clone)]
enum ParseFramesCommandError {
    UnknownCharacter(String), UnknownMove(String), WrongArguments,
}

fn parse_frames_command<'a>(args: Vec<String>, data: &'a GGSTDLData) -> Result<&'a Move, ParseFramesCommandError> {
    let mut iter = args.into_iter();

    let character_query = iter.next().ok_or(ParseFramesCommandError::WrongArguments)?;

    let move_query = iter.collect::<Vec<String>>().join(" ");
    if move_query.is_empty() {
        return Err(ParseFramesCommandError::WrongArguments);
    }

    match data.find_move(&character_query, &move_query) {
        Ok(move_found) => Ok(move_found),
        Err(e) => Err(match e {
            ggstdl::GGSTDLError::UnknownCharacter => ParseFramesCommandError::UnknownCharacter(character_query),
            ggstdl::GGSTDLError::UnknownMove => ParseFramesCommandError::UnknownMove(move_query),
        }),
    }
}

fn parse_message_to_command(raw: &str) -> Option<Command> {
    // ensure it only gets evaluated once
    lazy_static::lazy_static! {
        static ref MATCH: Regex = Regex::new(r"PRIVMSG #(.*) :(.*)").expect("Could not load command pasing regex");
    }

    let caps = MATCH.captures(raw)?;
    let channel = caps.get(1).map(|c| c.as_str())?.to_string();
    let msg = caps.get(2).map(|c| c.as_str())?;
    if msg.starts_with("!") {
        let mut split = msg.splitn(2, " ");
        let root = split.next()?.trim_end_matches("\r").to_string(); // if no args then this is here
        let args = match split.next() {
            Some(next) => next.trim_end_matches("\r").split(" ").map(|s| s.to_string()).collect::<Vec<String>>(),
            None => vec![]
        };
        return Some(Command {
            channel,
            command: root,
            args,
        });
    }

    None
}

fn format_msg(text: String, channel: String) -> Message {
    Message::Text(format!("PRIVMSG #{} :{}", channel, text))
}

fn format_move(fmt: &Move) -> String {
    format!("{}: dmg=({}) guard=({}) startup=({}) active=({}) recov=({}) block=({}) hit=({}) atklvl=({})", 
        fmt.input, fmt.damage, fmt.guard, fmt.startup, fmt.active, fmt.recovery, fmt.onblock, fmt.onhit, fmt.level)
}
