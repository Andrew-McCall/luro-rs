#![feature(option_result_contains)]
#![feature(const_mut_refs)]
#![feature(let_chains)]
use std::{
    collections::HashSet,
    env,
    sync::{atomic::AtomicUsize, Arc}
};

use config::{Config, Heck, Quotes, Secrets, Stories};
use database::add_discord_message;
use functions::furaffinity::event_furaffinity;
use poise::serenity_prelude::{self as serenity, Activity, OnlineStatus};
use regex::Regex;
use sled::Db;
use songbird::Songbird;
use tokio::sync::RwLock;
use tracing_subscriber::FmtSubscriber;

// Types
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;
type Command = poise::Command<Data, Error>;

// Important Constants, may be manually updated!
// TODO: A way to do these progmatically
const BOT_TOKEN: &str = "LURO_TOKEN";
const DATA_PATH: &str = "data/"; // Consider setting this to XDG_DATA_HOME on a production system
const CONFIG_FILE_PATH: &str = "data/config.toml";
const DATABASE_FILE_PATH: &str = "data/database";
const HECK_FILE_PATH: &str = "data/heck.toml";
const QUOTES_FILE_PATH: &str = "data/quotes.toml";
const SECRETS_FILE_PATH: &str = "data/secrets.toml";
const STORIES_FILE_PATH: &str = "data/stories.toml";
const FURSONA_FILE_PATH: &str = "data/fursona";

// Other Constants that will probably be changed to environment variables / config options
const FURAFFINITY_REGEX: &str = r"(?:https://)?(?:www\.)?furaffinity\.net/(?:view|full)/(?P<submission_id>\d+)/?|https://d\.(?:facdn|furaffinity).net/art/(?P<author>[\w\-.~:?#\[\]@!$&'()*+,;=%]+)/(?P<cdn_id>\d+)/(?P<original_cdn_id>\d*).\S*(?:gif|jpe?g|tiff?|png|webp|bmp)";
const SOURCE_FINDER_REGEX: &str = r"(?P<url>http[^\s>]+)";
const TIMEOUT_DURIATION: u64 = 12 * 60;

// Structs

// Trying to use best practices instead of Mutex (https://github.com/serenity-rs/serenity/blob/current/examples/e12_global_data/src/main.rs)
// e.g. Arc<RwLock<HashMap<String, u64>>> for read/write, and Arc<AtomicUsize> for read only.
pub struct Data {
    // General configuration stuffs
    config: Arc<RwLock<Config>>,
    // Work in progress database
    database: Arc<Db>,
    // Similar to stories, but aimed at users
    heck: Arc<RwLock<Heck>>,
    // Silly things users have said
    quotes: Arc<RwLock<Quotes>>,
    // Application secrets!
    secrets: Arc<Secrets>,
    // Stories, a bunch of silly messages effectively
    stories: Arc<RwLock<Stories>>,
    // Songbird instance for voice fun
    songbird: Arc<Songbird>,
    // Total commands ran in this instance
    command_total: Arc<RwLock<AtomicUsize>>
}

// Other modules
mod commands;
mod config;
mod database;
mod functions;
mod structs;

// Finally at Luro!
// ===============

// We had a fucky wucky
async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {error:?}"),
        poise::FrameworkError::Command { error, ctx } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
            ctx.send(|message| message.ephemeral(true).content(format!("Error in command `{}`: {:?}", ctx.command().name, error)))
                .await
                .expect("Could not send error to channel!");
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {e}")
            }
        }
    }
}

async fn event_listener(_ctx: &serenity::Context, event: &poise::Event<'_>, _framework: poise::FrameworkContext<'_, Data, Error>, _user_data: &Data) -> Result<(), Error> {
    match event {
        poise::Event::Ready { data_about_bot } => {
            let http = &_ctx.http;
            let api_version = data_about_bot.version;
            let bot_gateway = http.get_bot_gateway().await.unwrap();
            let t_sessions = bot_gateway.session_start_limit.total;
            let r_sessions = bot_gateway.session_start_limit.remaining;
            let bot_owner = http.get_current_application_info().await.unwrap().owner;

            println!("Successfully logged into Discord as the following user:");
            println!("Bot username: {}", data_about_bot.user.tag());
            println!("Bot user ID: {}", data_about_bot.user.id);
            println!("Bot owner: {}", bot_owner.tag());

            let guild_count = data_about_bot.guilds.len();

            println!("Connected to the Discord API (version {api_version}) with {r_sessions}/{t_sessions} sessions remaining.");
            println!("Connected to and serving a total of {guild_count} guild(s).");

            let presence_string = format!("on {guild_count} guilds | @luro help");
            _ctx.set_presence(Some(Activity::playing(&presence_string)), OnlineStatus::Online).await;
        }
        poise::Event::PresenceUpdate { new_data: _ } => {}
        poise::Event::InteractionCreate { interaction } => {
            let test = interaction.clone();
            match test.application_command() {
                Some(interaction_command) => {
                    println!("Event Listener: Data - {}", interaction_command.data.name)
                }
                None => println!("Event Listener: {}", interaction.id().0)
            };
        }
        poise::Event::TypingStart { event: _ } => {}
        poise::Event::GuildMemberUpdate { old_if_available: _, new: _ } => {}
        poise::Event::Message { new_message } => {
            if new_message.author.id == _framework.bot_id {
                return Ok(());
            }

            match add_discord_message(&_user_data.database, new_message.clone()) {
                Ok(_) => println!("Added message ID {} to database: {}", new_message.id.0, new_message.content),
                Err(err) => println!("Error while saving message to database: {err}")
            };

            let regex = Regex::new(FURAFFINITY_REGEX).unwrap();
            if let Some(fa_match) = regex.find(&new_message.content) {
                match event_furaffinity(_ctx, _framework, new_message).await {
                    Ok(_) => println!("Furaffinity: Regex matched - {}", fa_match.as_str()),
                    Err(err) => println!("Furaffinity: Regex failed with the following message - {err}")
                }
            }
        }

        _ => {
            println!("Got an event in listener: {:?}", event.name());
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let songbird = songbird::Songbird::serenity();
    let data = Data {
        config: RwLock::new(Config::get(CONFIG_FILE_PATH)).into(),
        database: sled::open(DATABASE_FILE_PATH).expect("Could not open / create database").into(),
        heck: RwLock::new(Heck::get(HECK_FILE_PATH)).into(),
        quotes: RwLock::new(Quotes::get(QUOTES_FILE_PATH)).into(),
        secrets: Secrets::get(SECRETS_FILE_PATH).into(),
        stories: RwLock::new(Stories::get(STORIES_FILE_PATH)).into(),
        songbird: songbird.clone(),
        command_total: RwLock::new(AtomicUsize::new(0)).into() // NOTE: Resets to zero on bot restart, by design
    };

    let token = match data.secrets.discord_token.clone() {
        Some(t) => t,
        None => match std::env::var(BOT_TOKEN) {
            Ok(environment_token) => environment_token,
            Err(err) => panic!("Congrats, you didn't set either {BOT_TOKEN} or include the token in the config file. Terminating on your sheer stupidity.\n{err}")
        }
    };

    env::set_var("RUST_LOG", "info,poise_basic_queue=trace,poise=debug,serenity=debug");
    let subscriber = FmtSubscriber::builder().with_target(false).finish();

    match tracing::subscriber::set_global_default(subscriber) {
        Ok(_) => println!("Loaded tracing subscriber"),
        Err(_) => panic!("Failed to load tracing subscriber!")
    };

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::commands(),
            owners: HashSet::from([serenity::UserId(97003404601094144), serenity::UserId(203994450467225600), serenity::UserId(105716849336938496)]),
            on_error: |error| Box::pin(on_error(error)),
            event_handler: |ctx, event, framework, user_data| {
                Box::pin(async move {
                    event_listener(ctx, event, framework, user_data).await?;
                    Ok(())
                })
            },
            pre_command: |ctx| {
                Box::pin(async move {
                    *ctx.data().command_total.write().await.get_mut() += 1;
                })
            },
            ..Default::default()
        })
        .setup(|_, _, _| Box::pin(async { Ok(data) }))
        .client_settings(move |f| f.voice_manager_arc(songbird))
        .token(token)
        .intents(
            serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT | serenity::GatewayIntents::GUILD_MEMBERS | serenity::GatewayIntents::GUILD_PRESENCES
        );

    framework.run().await.unwrap();
}
