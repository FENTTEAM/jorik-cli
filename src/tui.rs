use crate::api::{self, AudioFilters, EqualizerBand, FilterPayload, KaraokeOptions, LoopPayload, LowPassOptions, LyricsPayload, PlayPayload, QueuePayload, RotationOptions, SimplePayload, TimescaleOptions, TremoloOptions, TwentyFourSevenPayload, VibratoOptions, WsEvent, WsSubscribe, PlaybackState};
use crate::ascii::ASCII_LOGO;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap, BarChart, Bar, BarGroup, Gauge, Tabs},
    DefaultTerminal, Frame,
};
use ratatui::style::Stylize;
use reqwest::Client;
use serde_json::Value;
use std::{sync::Arc, time::{Duration, Instant}};
use tokio::sync::Mutex;
use tokio::time::{interval, timeout};
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;



// Theme Colors
struct Theme {
    bg: Color,
    border: Color,
    primary: Color,
    highlight: Color,
    text_secondary: Color,
}

fn get_theme(name: &str) -> Theme {
    match name {
        "Midnight" => Theme {
            bg: Color::Rgb(5, 5, 15),
            border: Color::Rgb(40, 40, 60),
            primary: Color::Rgb(100, 100, 255),
            highlight: Color::Rgb(150, 150, 255),
            text_secondary: Color::Rgb(120, 120, 140),
        },
        "Emerald" => Theme {
            bg: Color::Rgb(5, 15, 5),
            border: Color::Rgb(40, 60, 40),
            primary: Color::Rgb(50, 200, 50),
            highlight: Color::Rgb(100, 255, 100),
            text_secondary: Color::Rgb(120, 140, 120),
        },
        "Ruby" => Theme {
            bg: Color::Rgb(15, 5, 5),
            border: Color::Rgb(60, 40, 40),
            primary: Color::Rgb(200, 50, 50),
            highlight: Color::Rgb(255, 100, 100),
            text_secondary: Color::Rgb(140, 120, 120),
        },
        "Ocean" => Theme {
            bg: Color::Rgb(5, 10, 20),
            border: Color::Rgb(40, 60, 100),
            primary: Color::Rgb(50, 150, 255),
            highlight: Color::Rgb(100, 200, 255),
            text_secondary: Color::Rgb(120, 130, 160),
        },
        "Synthwave" => Theme {
            bg: Color::Rgb(20, 10, 30),
            border: Color::Rgb(100, 40, 100),
            primary: Color::Rgb(255, 50, 255),
            highlight: Color::Rgb(255, 150, 50), // Orange highlight
            text_secondary: Color::Rgb(160, 120, 180),
        },
        "Sepia" => Theme {
            bg: Color::Rgb(30, 25, 20),
            border: Color::Rgb(80, 70, 60),
            primary: Color::Rgb(180, 140, 100),
            highlight: Color::Rgb(220, 180, 140),
            text_secondary: Color::Rgb(140, 130, 120),
        },
        _ => Theme { // Default Jorik Purple
            bg: Color::Rgb(15, 15, 25),
            border: Color::Rgb(60, 60, 80),
            primary: JORIK_PURPLE,
            highlight: JORIK_HIGHLIGHT,
            text_secondary: Color::Rgb(150, 150, 170),
        },
    }
}

// Approx color from the logo
const JORIK_PURPLE: Color = Color::Rgb(130, 110, 230); // Soft purple/indigo
const JORIK_HIGHLIGHT: Color = Color::Rgb(160, 140, 250);

#[derive(PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

#[derive(PartialEq, Clone, Copy)]
enum View {
    Main,
    Menu,
    Lyrics,
    FilterMenu,
    AuthMenu,
    AuthResult,
    LoginRequired,
    Settings,
    Debug,
    AppInfo,
}

#[derive(PartialEq, Clone, Copy)]
enum SettingsField {
    Host,
    Offset,
    Theme,
    VizStyle,
    Layout,
}

struct App {
    client: Client,
    base_url: String,
    token: Option<String>,
    guild_id: Option<String>,
    user_id: Option<String>,
    
    queue: Vec<String>,
    current_track: Option<String>,
    error_message: Option<String>,
    fatal_error: Option<String>,
    loop_mode: String, // "off", "track", "queue"
    is_loading: bool,
    
    input: String,
    input_mode: InputMode,
    view: View,
    
    menu_state: ListState,
    menu_items: Vec<&'static str>,
    
    filter_state: ListState,
    filter_items: Vec<&'static str>,
    
    auth_menu_state: ListState,
    auth_menu_items: Vec<&'static str>,

    lyrics_text: Option<String>,
    lyrics_scroll: u16,
    
    auth_info_text: Option<String>,

    // Real-time data
    spectrogram: Option<Vec<Vec<u8>>>,
    elapsed_ms: u64,
    duration_ms: u64,
    paused: bool,
    last_state_update: Instant,

    settings_input: String,
    offset_input: String,
    theme: String,
    viz_style: String,
    layout: String,
    settings_field: SettingsField,
    is_settings_editing: bool,
    needs_reconnect: bool,
    visualizer_offset: i64,

    debug_logs: Vec<String>,
    ws_connected: bool,
    ws_connecting: bool,
    ws_sender: Option<tokio::sync::mpsc::UnboundedSender<Message>>,

    smoothed_bars: Vec<f32>,
}

impl App {
    fn new(
        client: Client,
        settings: api::Settings,
        token: Option<String>,
        guild_id: Option<String>,
        user_id: Option<String>,
    ) -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));
        
        let mut filter_state = ListState::default();
        filter_state.select(Some(0));

        let mut auth_menu_state = ListState::default();
        auth_menu_state.select(Some(0));
        
        let view = if token.is_some() { View::Main } else { View::LoginRequired };

        Self {
            client,
            base_url: settings.base_url.clone(),
            token,
            guild_id,
            user_id,
            queue: Vec::new(),
            current_track: None,
            error_message: None,
            fatal_error: None,
            loop_mode: "off".to_string(),
            is_loading: false,
            input: String::new(),
            input_mode: InputMode::Normal,
            view,
            menu_state,
            menu_items: vec![
                " [+] Skip ", " [||] Pause/Resume ", " [X] Stop ", " [/] Shuffle ", 
                " [C] Clear Queue ", " [T] Loop Track ", " [Q] Loop Queue ", " [.] Loop Off ",
                " [24/7] Mode Toggle ", " [F] Filters... ", " [L] Lyrics ", " [P] Play Turip ",
                " [A] Auth ", " [S] Settings ", " [!] Exit TUI "
            ],
            filter_state,
            filter_items: vec![
                "Clear", "Bassboost", "Nightcore", "Vaporwave", 
                "8D", "Soft", "Tremolo", "Vibrato", "Karaoke"
            ],
            auth_menu_state,
            auth_menu_items: vec!["Login", "Signout", "Info"],
            lyrics_text: None,
            lyrics_scroll: 0,
            auth_info_text: None,
            spectrogram: None,
            elapsed_ms: 0,
            duration_ms: 0,
            paused: true,
            last_state_update: Instant::now(),
            settings_input: settings.base_url.clone(),
            offset_input: settings.visualizer_offset.to_string(),
            theme: settings.theme,
            viz_style: settings.visualizer_style,
            layout: settings.layout,
            settings_field: SettingsField::Host,
            is_settings_editing: false,
            needs_reconnect: false,
            visualizer_offset: settings.visualizer_offset,
            debug_logs: Vec::new(),
            ws_connected: false,
            ws_connecting: false,
            ws_sender: None,
            smoothed_bars: vec![0.0; 64],
        }
    }

    fn log(&mut self, msg: impl Into<String>) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.debug_logs.push(format!("[{}] {}", timestamp, msg.into()));
        if self.debug_logs.len() > 100 {
            self.debug_logs.remove(0);
        }
    }

    fn save_spectrogram(&mut self) {
        let spec = match &self.spectrogram {
            Some(s) => s,
            None => {
                self.log("Save failed: No spectrogram data available.");
                return;
            }
        };

        let desktop = match dirs::desktop_dir() {
            Some(d) => d,
            None => {
                self.log("Save failed: Could not find Desktop directory.");
                return;
            }
        };

        let filename = format!(
            "spectrogram_{}.json",
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        );
        let path = desktop.join(filename);

        match serde_json::to_string_pretty(spec) {
            Ok(json) => {
                if let Ok(_) = std::fs::write(&path, json) {
                    self.log(format!("Spectrogram saved to: {:?}", path));
                } else {
                    self.log("Save failed: Could not write to file.");
                }
            }
            Err(_) => {
                self.log("Save failed: Could not serialize spectrogram.");
            }
        }
    }

    fn parse_queue_response(&mut self, json: &Value) {
        // Handle nested queue object if present
        let target = if let Some(queue) = json.get("queue") {
            queue
        } else {
            json
        };

        // Capture guild_id if provided by server
        if let Some(gid) = json.get("guild_id").and_then(|v| v.as_str()) {
            if self.guild_id.is_none() {
                self.log(format!("Discovered Guild ID: {}", gid));
            }
            self.guild_id = Some(gid.to_string());
        } else if let Some(gid) = json.get("guildId").and_then(|v| v.as_str()) {
            if self.guild_id.is_none() {
                self.log(format!("Discovered Guild ID: {}", gid));
            }
            self.guild_id = Some(gid.to_string());
        }

        if let Some(current) = target.get("current").and_then(|v| v.as_object()) {
            let title = current.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown");
            let author = current.get("author").and_then(|v| v.as_str()).unwrap_or("");
            self.current_track = Some(format!("{} - {}", title, author));
        } else {
            // Only clear current_track if we are sure we are looking at a queue object
            if target.get("current").is_some() || target.get("upcoming").is_some() {
                self.current_track = None;
            }
        }

        if let Some(upcoming) = target.get("upcoming").and_then(|v| v.as_array()) {
            self.queue.clear();
            for item in upcoming {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown");
                let author = item.get("author").and_then(|v| v.as_str()).unwrap_or("");
                self.queue.push(format!("{} - {}", title, author));
            }
        }
    }

    fn update_realtime(&mut self) {
        if self.current_track.is_some() && !self.paused {
            let now = Instant::now();
            let delta = now.duration_since(self.last_state_update).as_millis() as u64;
            self.elapsed_ms += delta;
            self.last_state_update = now;
            
            if self.duration_ms > 0 && self.elapsed_ms > self.duration_ms {
                self.elapsed_ms = self.duration_ms;
            }

            // Smoothing logic
            if let Some(spec) = &self.spectrogram {
                let adjusted_ms = self.elapsed_ms.saturating_add_signed(self.visualizer_offset);
                let frame_index = (adjusted_ms as f64 / 42.66).floor() as usize;
                if frame_index < spec.len() {
                    let target_bars = &spec[frame_index];
                    for i in 0..64.min(target_bars.len()) {
                        let target = target_bars[i] as f32;
                        let current = self.smoothed_bars[i];
                        
                        // Variable noise floor: higher for sub-bass to ignore rumble
                        let floor = if i < 3 { 60.0 } else { 30.0 };
                        let raw_signal = (target - floor).max(0.0);
                        
                        // Simple direct scaling
                        let gain = if i == 0 { 0.1 } else { 0.6 };
                        let scaled_target = (raw_signal * gain).min(100.0);

                        // Factors adjusted for 60fps
                        if scaled_target > current {
                            self.smoothed_bars[i] = current + (scaled_target - current) * 0.4; 
                        } else {
                            self.smoothed_bars[i] = current - (current - scaled_target) * 0.15;
                        }
                    }
                }
            }
        } else {
            self.last_state_update = Instant::now();
            // Fade out bars when idle
            for i in 0..64 {
                self.smoothed_bars[i] *= 0.95;
            }
        }
    }
}

// Spawning helpers
async fn async_fetch_queue(app_arc: Arc<Mutex<App>>) {
    let (client, url, token, payload) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        let payload = QueuePayload {
            action: "queue",
            guild_id: app.guild_id.clone(),
            user_id: app.user_id.clone(),
            limit: 20,
            offset: 0,
        };
        let url = api::build_url(&app.base_url, "/webhook/audio");
        (app.client.clone(), url, app.token.clone(), payload)
    };

    let mut req = client.post(&url).json(&payload);
    if let Some(bearer) = &token {
        req = req.bearer_auth(bearer);
    }

    let result = req.send().await;
    
    let mut app = app_arc.lock().await;
    app.is_loading = false;
    match result {
        Ok(resp) => {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<Value>().await {
                    app.parse_queue_response(&json);
                    app.error_message = None;
                }
            } else {
                 let text = resp.text().await.unwrap_or_default();
                 
                 let mut handled = false;
                 if let Ok(json_err) = serde_json::from_str::<Value>(&text) {
                     if json_err.get("error").and_then(|v| v.as_str()) == Some("bad_request") &&
                        json_err.get("message").and_then(|v| v.as_str()) == Some("user_not_in_voice_channel_or_guild_unknown") {
                            app.fatal_error = Some("User not in voice channel or guild unknown.\n\nPress 'r' to reload.".to_string());
                            handled = true;
                     }
                 }

                 if !handled {
                     if text.contains("guild_id is required") {
                         app.error_message = Some("Not connected to a voice channel or Guild ID missing.".to_string());
                     } else {
                         app.error_message = Some(format!("Error: {}", text));
                     }
                 }
            }
        }
        Err(e) => {
            app.error_message = Some(format!("Network error: {}", e));
        }
    }
}

async fn async_play_track(app_arc: Arc<Mutex<App>>, query: String) {
    let (ws_sender, ws_connected, client, url, token, payload) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        let payload = PlayPayload {
            action: "play",
            guild_id: app.guild_id.clone(),
            channel_id: None,
            query: api::clean_query(&query),
            user_id: app.user_id.clone(),
            requested_by: None,
            avatar_url: None,
        };
        let url = api::build_url(&app.base_url, "/webhook/audio");
        (app.ws_sender.clone(), app.ws_connected, app.client.clone(), url, app.token.clone(), payload)
    };

    if ws_connected {
        if let Some(sender) = ws_sender {
            let ws_action = api::WsAction {
                event_type: "action",
                id: format!("play-{}", chrono::Local::now().timestamp_millis()),
                payload: &payload,
            };
            if let Ok(json) = serde_json::to_string(&ws_action) {
                if let Ok(_) = sender.send(Message::Text(json.into())) {
                    // Success sending via WS
                    // We still set is_loading to false after a bit, or let the WS event handle it.
                    // Actually, WS event will refresh the queue anyway.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let mut app = app_arc.lock().await;
                    app.is_loading = false;
                    return;
                }
            }
        }
    }

    // Fallback to REST
    let mut req = client.post(&url).json(&payload);
    if let Some(bearer) = &token {
        req = req.bearer_auth(bearer);
    }

    let _ = req.send().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    async_fetch_queue(app_arc).await;
}

async fn async_fetch_lyrics(app_arc: Arc<Mutex<App>>) {
    let (ws_sender, ws_connected, client, url, token, payload) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        let payload = LyricsPayload {
            action: "lyrics".to_string(),
            guild_id: app.guild_id.clone(),
            user_id: app.user_id.clone(),
        };
        let url = api::build_url(&app.base_url, "/webhook/audio");
        (app.ws_sender.clone(), app.ws_connected, app.client.clone(), url, app.token.clone(), payload)
    };

    if ws_connected {
        if let Some(sender) = ws_sender {
            let ws_action = api::WsAction {
                event_type: "action",
                id: format!("lyrics-{}", chrono::Local::now().timestamp_millis()),
                payload: &payload,
            };
            if let Ok(json) = serde_json::to_string(&ws_action) {
                if let Ok(_) = sender.send(Message::Text(json.into())) {
                }
            }
        }
    }

    let mut req = client.post(&url).json(&payload);
    if let Some(bearer) = &token {
        req = req.bearer_auth(bearer);
    }

    let result = req.send().await;
    
    let mut app = app_arc.lock().await;
    app.view = View::Lyrics;
    app.lyrics_scroll = 0;
    app.is_loading = false;
    
    match result {
        Ok(resp) => {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(data) = json.get("data").and_then(|v| v.as_object()) {
                    let mut output = String::new();
                    if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
                        output.push_str(text);
                    } else if let Some(lines) = data.get("lines").and_then(|v| v.as_array()) {
                        for line in lines {
                            let text = line.get("line").and_then(|v| v.as_str()).unwrap_or("");
                            output.push_str(&format!("{}\n", text));
                        }
                    }
                    if output.trim().is_empty() {
                         app.lyrics_text = Some("No lyrics found.".to_string());
                    } else {
                         app.lyrics_text = Some(output);
                    }
                } else {
                    app.lyrics_text = Some("No lyrics found.".to_string());
                }
            } else {
                app.lyrics_text = Some("Failed to parse lyrics.".to_string());
            }
        }
        Err(e) => {
            app.lyrics_text = Some(format!("Failed to fetch lyrics: {}", e));
        }
    }
}

async fn async_simple_command<T: serde::Serialize + Send + Sync + 'static>(app_arc: Arc<Mutex<App>>, endpoint: String, payload: T) {
    let (ws_sender, ws_connected, client, url, token) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        let url = api::build_url(&app.base_url, &endpoint);
        (app.ws_sender.clone(), app.ws_connected, app.client.clone(), url, app.token.clone())
    };

    if ws_connected && endpoint.contains("/webhook/audio") {
        if let Some(sender) = ws_sender {
            let ws_action = api::WsAction {
                event_type: "action",
                id: format!("cmd-{}", chrono::Local::now().timestamp_millis()),
                payload: &payload,
            };
            if let Ok(json) = serde_json::to_string(&ws_action) {
                if let Ok(_) = sender.send(Message::Text(json.into())) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let mut app = app_arc.lock().await;
                    app.is_loading = false;
                    return;
                }
            }
        }
    }

    let mut req = client.post(&url).json(&payload);
    if let Some(bearer) = &token {
        req = req.bearer_auth(bearer);
    }

    let _ = req.send().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    async_fetch_queue(app_arc).await;
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn async_auth_login(app_arc: Arc<Mutex<App>>) {
    let (base_url, is_login_required_screen) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        app.auth_info_text = Some("Initializing login...".to_string());
        
        let is_login_required = app.view == View::LoginRequired;
        
        // If we are NOT on the LoginRequired screen (meaning we are in the Auth Menu), 
        // switch to AuthResult to show the popup.
        // If we ARE on LoginRequired, we do NOTHING to the view, staying on that screen.
        if !is_login_required {
            app.view = View::AuthResult;
        }
        
        (app.base_url.clone(), is_login_required)
    };

    let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
        Ok(l) => l,
        Err(e) => {
            let mut app = app_arc.lock().await;
            app.is_loading = false;
            app.auth_info_text = Some(format!("Failed to bind listener: {}", e));
            return;
        }
    };

    let local_addr = match listener.local_addr() {
        Ok(a) => a,
        Err(e) => {
            let mut app = app_arc.lock().await;
            app.is_loading = false;
            app.auth_info_text = Some(format!("Failed to get local addr: {}", e));
            return;
        }
    };

    let callback_url = format!("http://{}/oauth-callback", local_addr);
    
    let mut auth_url = match reqwest::Url::parse(&api::build_url(&base_url, "/authorize")) {
        Ok(u) => u,
        Err(e) => {
            let mut app = app_arc.lock().await;
            app.is_loading = false;
            app.auth_info_text = Some(format!("Invalid base URL: {}", e));
            return;
        }
    };
    
    auth_url.query_pairs_mut().append_pair("callback", &callback_url);

    {
        let mut app = app_arc.lock().await;
        app.auth_info_text = Some(format!("Opening browser...\n\nIf it doesn't open, visit:\n{}", auth_url.as_str()));
    }
    
    let _ = open::that(auth_url.as_str());

    // Wait for callback (120s timeout)
    match timeout(Duration::from_secs(120), listener.accept()).await {
        Ok(Ok((mut stream, _addr))) => {
            let mut buf = vec![0u8; 8192];
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    let mut app = app_arc.lock().await;
                    app.is_loading = false;
                    app.auth_info_text = Some(format!("Error reading callback: {}", e));
                    return;
                }
            };
            
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            let path = first_line.split_whitespace().nth(1).unwrap_or("");
            
            // Prepend a scheme+host so `Url::parse` can parse query params.
            if let Ok(parsed) = reqwest::Url::parse(&format!("http://localhost{}", path)) {
                let token_pair = parsed.query_pairs().find(|(k, _)| k == "token");
                let avatar_pair = parsed.query_pairs().find(|(k, _)| k == "avatar");
                let username_pair = parsed.query_pairs().find(|(k, _)| k == "username");
                
                if let Some((_, v)) = token_pair {
                    let token = v.into_owned();
                    let token_trim = token.trim().to_string();
                    if token_trim.is_empty() {
                        let body = "Missing token";
                        let resp = format!(
                            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        
                        let mut app = app_arc.lock().await;
                        app.is_loading = false;
                        app.auth_info_text = Some("No token provided in callback.".to_string());
                        return;
                    }

                    let avatar_val = avatar_pair.map(|(_, val)| val.into_owned());
                    let username_val = username_pair.map(|(_, val)| val.into_owned());

                    if let Err(e) = api::save_token(&token_trim, avatar_val.as_deref(), username_val.as_deref()) {
                        let mut app = app_arc.lock().await;
                        app.is_loading = false;
                        app.auth_info_text = Some(format!("Failed to save token: {}", e));
                        return;
                    }

                    // Build a small, readable success page and kick off confetti animation.
                    let escaped_username = username_val
                        .as_deref()
                        .map(escape_html)
                        .unwrap_or_else(|| "User".to_string());
                    let escaped_avatar = avatar_val.as_deref().map(escape_html);
                    let saved_path_html = if let Some(path) = api::config_file_path() {
                        format!(
                            "<p>Saved to <code>{}</code></p>",
                            escape_html(&path.display().to_string())
                        )
                    } else {
                        "".to_string()
                    };

                    let mut body = String::new();
                    body.push_str(
                        "<!doctype html><html><head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"/><title>Authorization complete</title><style>",
                    );
                    body.push_str("body{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",Roboto,\"Helvetica Neue\",Arial, sans-serif;background:#2f3136;color:#dcddde;margin:0;padding:0;display:flex;align-items:center;justify-content:center;height:100vh}");
                    body.push_str(".container{max-width:560px;width:100%;padding:28px;background:#36393f;border-radius:12px;box-shadow:0 6px 20px rgba(0,0,0,0.6)}");
                    body.push_str(
                        ".header{display:flex;align-items:center;gap:16px;margin-bottom:18px}",
                    );
                    body.push_str(".badge{width:56px;height:56px;display:flex;align-items:center;justify-content:center;border-radius:50%;background:#2f3136}");
                    body.push_str(".check{width:34px;height:34px;border-radius:50%;background:#43b581;color:#fff;display:flex;align-items:center;justify-content:center;font-weight:700;font-size:16px}");
                    body.push_str(".avatar{width:56px;height:56px;border-radius:50%;object-fit:cover;border:2px solid rgba(0,0,0,0.4)}");
                    body.push_str(".user{font-size:16px;font-weight:600;margin:0;color:#fff}");
                    body.push_str(".sp{color:#b9bbbe;font-size:13px;margin-top:4px}");
                    body.push_str(".path{display:inline-block;background:#2f3136;padding:6px 8px;border-radius:6px;color:#b9bbbe;font-family:monospace;margin-top:8px}");
                    body.push_str(
                        "</style></head><body><div class=\"container\"><div class=\"header\">",
                    );
                    if let Some(avatar) = &escaped_avatar {
                        body.push_str(&format!(
                            r#"<img class="avatar" src="{}" alt="avatar"/>"#,
                            avatar
                        ));
                    } else {
                        body.push_str(r#"<div class="badge"><div class="check">✓</div></div>"#);
                    }
                    body.push_str(&format!(
                        r#"<div><div class="user">{}</div><div class="sp">Authorization complete</div>{}</div>"#,
                        escaped_username, saved_path_html
                    ));
                    body.push_str(r#"</div><div><p class="sp">Token saved to your config. You may close this window.</p></div>"#);

                    // confetti
                    body.push_str(r#"<script src="https://cdn.jsdelivr.net/npm/canvas-confetti@1.6.0/dist/confetti.browser.min.js"></script>"#);
                    body.push_str(
                        r#"<script>
  const duration = 15 * 1000,
    animationEnd = Date.now() + duration,
    defaults = { startVelocity: 30, spread: 360, ticks: 60, zIndex: 0 };

  function randomInRange(min, max) {
    return Math.random() * (max - min) + min;
  }

  const interval = setInterval(function() {
    const timeLeft = animationEnd - Date.now();

    if (timeLeft <= 0) {
      return clearInterval(interval);
    }

    const particleCount = 50 * (timeLeft / duration);

    confetti(
      Object.assign({}, defaults, {
        particleCount,
        origin: { x: randomInRange(0.1, 0.3), y: Math.random() - 0.2 },
      })
    );
    confetti(
      Object.assign({}, defaults, {
        particleCount,
        origin: { x: randomInRange(0.7, 0.9), y: Math.random() - 0.2 },
      })
    );
  }, 250);
</script>"#,
                    );
                    body.push_str("</div></body></html>");

                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;

                    {
                        let mut app = app_arc.lock().await;
                        app.is_loading = false;
                        app.token = Some(token_trim.clone());
                        app.auth_info_text = Some(format!("Login Successful!\n\nUser: {}\nToken saved.", username_val.unwrap_or_default()));
                    }

                    // Small delay to ensure stability
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    // Refresh data before switching view
                    async_fetch_queue(app_arc.clone()).await;

                    let mut app = app_arc.lock().await;
                    // Only transition to Main if we were on the LoginRequired screen.
                    if is_login_required_screen {
                        app.view = View::Main;
                    }
                } else {                    let body = "No token in callback";
                    let resp = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    
                    let mut app = app_arc.lock().await;
                    app.is_loading = false;
                    app.auth_info_text = Some("Login failed: Missing token in callback.".to_string());
                }
            }
        }
        _ => {
            let mut app = app_arc.lock().await;
            app.is_loading = false;
            app.auth_info_text = Some("Login timed out.".to_string());
        }
    }
}

async fn async_auth_signout(app_arc: Arc<Mutex<App>>) {
    let (client, base_url, token) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        app.view = View::AuthResult;
        app.auth_info_text = Some("Signing out...".to_string());
        (app.client.clone(), app.base_url.clone(), app.token.clone())
    };

    if let Some(tok) = token {
        let url = api::build_url(&base_url, "/webhook/auth/revoke");
        let _ = client.post(&url).bearer_auth(tok).send().await;
    }

    // Remove local file
    if let Some(path) = api::config_file_path() {
        if path.exists() {
             let _ = std::fs::remove_file(path);
        }
    }

    let mut app = app_arc.lock().await;
    app.is_loading = false;
    app.token = None;
    app.auth_info_text = None;
    app.view = View::LoginRequired;
}

async fn spawn_websocket(app_arc: Arc<Mutex<App>>, mut ws_rx: tokio::sync::mpsc::UnboundedReceiver<Message>) {
    let mut last_waiting_log = Instant::now();
    
    loop {
        let (base_url, token, guild_id) = {
            let app = app_arc.lock().await;
            (app.base_url.clone(), app.token.clone(), app.guild_id.clone())
        };

        if token.is_none() || guild_id.is_none() {
            if last_waiting_log.elapsed() > Duration::from_secs(10) {
                let mut app = app_arc.lock().await;
                if token.is_none() {
                    app.log("WS waiting for token...");
                } else if guild_id.is_none() {
                    app.log("WS waiting for Guild ID (join a voice channel or specify --guild-id)...");
                }
                last_waiting_log = Instant::now();
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        let token = token.unwrap();
        let guild_id = guild_id.unwrap();

        let ws_url = match Url::parse(&base_url) {
            Ok(u) => {
                let scheme = if u.scheme() == "https" { "wss" } else { "ws" };
                let mut u = u;
                u.set_scheme(scheme).ok();
                u.set_path("/ws");
                u.query_pairs_mut().append_pair("token", &token);
                u
            }
            Err(e) => {
                let mut app = app_arc.lock().await;
                app.log(format!("WS URL Parse Error: {}", e));
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        {
            let mut app = app_arc.lock().await;
            app.log(format!("WS Connecting to {}", ws_url));
            app.ws_connected = false;
            app.ws_connecting = true;
        }

        match connect_async(ws_url.as_str()).await {
            Ok((mut ws_stream, _)) => {
                {
                    let mut app = app_arc.lock().await;
                    app.log("WS Connected");
                    app.ws_connected = true;
                    app.ws_connecting = false;
                }
                
                let sub = WsSubscribe {
                    event_type: "subscribe",
                    guild_id: guild_id.clone(),
                };
                if let Ok(json) = serde_json::to_string(&sub) {
                    let _ = ws_stream.send(Message::Text(json.into())).await;
                }

                loop {
                    tokio::select! {
                        msg = ws_stream.next() => {
                            match msg {
                                Some(Ok(Message::Text(text))) => {
                                    if let Ok(event) = serde_json::from_str::<WsEvent>(&text) {
                                        let mut app = app_arc.lock().await;
                                        app.log(format!("WS Event: {}", event.event_type));
                                        
                                        match event.event_type.as_str() {
                                            "spectrogram_update" => {
                                                if event.guild_id.as_deref() == app.guild_id.as_deref() {
                                                    if let Some(data) = event.data {
                                                        if let Ok(spectrogram) = serde_json::from_value::<Vec<Vec<u8>>>(data) {
                                                            app.log(format!("Received Spectrogram ({} frames)", spectrogram.len()));
                                                            app.spectrogram = Some(spectrogram);
                                                        }
                                                    }
                                                }
                                            }
                                            "state_update" | "initial_state" => {
                                                if event.guild_id.as_deref() == app.guild_id.as_deref() {
                                                    if let Some(data) = &event.data {
                                                        app.parse_queue_response(data);
                                                    }

                                                    // Check both root and data.playback for robustness
                                                    let playback = event.playback.clone().or_else(|| {
                                                        event.data.as_ref()
                                                            .and_then(|d| d.get("playback"))
                                                            .and_then(|p| serde_json::from_value::<PlaybackState>(p.clone()).ok())
                                                    });

                                                    if let Some(playback) = playback {
                                                        if playback.elapsed_ms % 5000 < 500 { // Log every ~5 seconds
                                                            app.log(format!("State Update: elapsed={}ms, paused={}", playback.elapsed_ms, playback.paused));
                                                        }
                                                        if app.elapsed_ms == 0 && playback.elapsed_ms > 0 {
                                                            app.log(format!("Synced playback to {}ms", playback.elapsed_ms));
                                                        }
                                                        app.elapsed_ms = playback.elapsed_ms;
                                                        app.duration_ms = playback.duration_ms;
                                                        app.paused = playback.paused;
                                                        app.last_state_update = Instant::now();
                                                        if let Some(spec) = playback.spectrogram {
                                                            app.log(format!("Received Spectrogram in state ({} frames)", spec.len()));
                                                            app.spectrogram = Some(spec);
                                                        }
                                                    }
                                                }
                                            }
                                            "queue_update" => {
                                                if event.guild_id.as_deref() == app.guild_id.as_deref() {
                                                    app.log("Received Queue Update");
                                                    if let Some(data) = event.data {
                                                        app.parse_queue_response(&data);
                                                    } else {
                                                        // Fallback to REST if data is missing
                                                        tokio::spawn(async_fetch_queue(app_arc.clone()));
                                                    }
                                                }
                                            }
                                            "track_start" | "track_end" | "player_update" => {
                                                if event.guild_id.as_deref() == app.guild_id.as_deref() {
                                                    app.log(format!("WS Event: {}, refreshing queue", event.event_type));
                                                    // Trigger a full REST refresh to get the latest queue state
                                                    tokio::spawn(async_fetch_queue(app_arc.clone()));
                                                }
                                            }
                                            "action_response" => {
                                                let success = event.success.unwrap_or(false);
                                                let id = event.id.as_deref().unwrap_or("unknown");
                                                app.log(format!("WS Action Response [{}]: success={}", id, success));
                                            }
                                            _ => {
                                                app.log(format!("WS Unhandled Event: {}", event.event_type));
                                            }
                                        }
                                    } else {
                                        let mut app = app_arc.lock().await;
                                        app.log(format!("WS Unparsed Message: {}", text));
                                    }
                                }
                                Some(Err(e)) => {
                                    let mut app = app_arc.lock().await;
                                    app.log(format!("WS Error: {}", e));
                                    break;
                                }
                                None => {
                                    let mut app = app_arc.lock().await;
                                    app.log("WS Closed");
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Some(out_msg) = ws_rx.recv() => {
                            if let Err(e) = ws_stream.send(out_msg).await {
                                let mut app = app_arc.lock().await;
                                app.log(format!("WS Send Error: {}", e));
                                break;
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {
                            let mut app = app_arc.lock().await;
                            if app.needs_reconnect {
                                app.log("WS Forcing reconnect due to settings change");
                                app.needs_reconnect = false;
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let mut app = app_arc.lock().await;
                app.log(format!("WS Connection Failed: {}", e));
                app.ws_connecting = false;
            }
        }
        
        {
            let mut app = app_arc.lock().await;
            app.ws_connected = false;
            app.ws_connecting = false;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

pub async fn run(
    settings: api::Settings,
    token: Option<String>,
    guild_id: Option<String>,
    user_id: Option<String>,
) -> Result<()> {
    let client = Client::builder()
        .user_agent("jorik-cli-tui")
        .timeout(Duration::from_secs(10))
        .build()?;

    let (ws_tx, ws_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    let mut app_struct = App::new(client, settings, token, guild_id, user_id);
    app_struct.ws_sender = Some(ws_tx);
    
    let app = Arc::new(Mutex::new(app_struct));
    
    // Initial fetch
    tokio::spawn(async_fetch_queue(app.clone()));
    tokio::spawn(spawn_websocket(app.clone(), ws_rx));

    let app_clone = app.clone();
    tokio::spawn(async move {
        // Poll every 20 seconds for safety if WS misses an update
        let mut interval = interval(Duration::from_secs(20));
        loop {
            interval.tick().await;
            async_fetch_queue(app_clone.clone()).await;
        }
    });

    let mut terminal = ratatui::init();
    let res = run_loop(&mut terminal, app).await;
    ratatui::restore();
    res
}

async fn run_loop(terminal: &mut DefaultTerminal, app_arc: Arc<Mutex<App>>) -> Result<()> {
    loop {
        {
            let mut app = app_arc.lock().await;
            app.update_realtime();
            terminal.draw(|f| ui(f, &mut *app))?;
        }

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let mut app = app_arc.lock().await;

                    if app.fatal_error.is_some() {
                        if let KeyCode::Char('r') | KeyCode::Char('к') = key.code {
                            app.fatal_error = None;
                            app.error_message = None;
                            drop(app);
                            tokio::spawn(async_fetch_queue(app_arc.clone()));
                        }
                        continue;
                    }
                    
                    if app.input_mode == InputMode::Editing {
                        handle_editing_keys(&mut *app, key, app_arc.clone());
                        continue;
                    }

                    if app.is_settings_editing {
                        handle_settings_keys(&mut *app, key, app_arc.clone());
                        continue;
                    }

                    // Global Tab Switching (1-4)
                    match key.code {
                        KeyCode::Char('1') => { app.view = View::Main; continue; }
                        KeyCode::Char('2') => { 
                            if app.view != View::Lyrics {
                                tokio::spawn(async_fetch_lyrics(app_arc.clone()));
                            }
                            app.view = View::Lyrics; 
                            continue; 
                        }
                        KeyCode::Char('3') => { 
                            app.settings_input = app.base_url.clone();
                            app.view = View::Settings; 
                            continue; 
                        }
                        KeyCode::Char('4') => { app.view = View::Debug; continue; }
                        _ => {}
                    }

                    // Global Quit (q) - except in Settings where it might be typed
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('й')) && app.view != View::Settings {
                        return Ok(());
                    }

                    // View-Specific Handlers
                    match app.view {
                        View::Main => handle_player_keys(&mut *app, key, app_arc.clone()),
                        View::Lyrics => handle_lyrics_keys(&mut *app, key),
                        View::Settings => handle_settings_keys(&mut *app, key, app_arc.clone()),
                        View::Debug => handle_debug_keys(&mut *app, key),
                        View::Menu => { if handle_menu_keys(&mut *app, key, app_arc.clone())? { return Ok(()); } },
                        View::FilterMenu => handle_filter_menu_keys(&mut *app, key, app_arc.clone()),
                        View::AuthMenu => handle_auth_menu_keys(&mut *app, key, app_arc.clone()),
                        View::AuthResult => {
                            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace) {
                                app.view = View::AuthMenu;
                            }
                        }
                        View::AppInfo => {
                            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace | KeyCode::Char('i') | KeyCode::Char('ш')) {
                                app.view = View::Main;
                            }
                        }
                        View::LoginRequired => {
                            if key.code == KeyCode::Enter {
                                tokio::spawn(async_auth_login(app_arc.clone()));
                            } else if key.code == KeyCode::Char('\\') {
                                app.settings_input = app.base_url.clone();
                                app.view = View::Settings;
                            } else if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('й')) {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }
}

fn handle_editing_keys(app: &mut App, key: event::KeyEvent, app_arc: Arc<Mutex<App>>) {
    match key.code {
        KeyCode::Enter => {
            let query = app.input.clone();
            app.input.clear();
            app.input_mode = InputMode::Normal;
            tokio::spawn(async_play_track(app_arc, query));
        }
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.input.clear();
        }
        KeyCode::Char(c) => app.input.push(c),
        KeyCode::Backspace => { app.input.pop(); }
        _ => {}
    }
}

fn handle_player_keys(app: &mut App, key: event::KeyEvent, app_arc: Arc<Mutex<App>>) {
    match key.code {
        KeyCode::Char('r') | KeyCode::Char('к') => {
            tokio::spawn(async_fetch_queue(app_arc));
        }
        KeyCode::Tab => app.view = View::Menu,
        KeyCode::Enter => app.input_mode = InputMode::Editing,
        KeyCode::Char('l') | KeyCode::Char('д') => {
            let new_mode = match app.loop_mode.as_str() {
                "off" => "track",
                "track" => "queue",
                "queue" => "off",
                _ => "off",
            };
            app.loop_mode = new_mode.to_string();
            tokio::spawn(async_simple_command(app_arc, "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: new_mode.to_string() }));
        }
        KeyCode::Char('s') | KeyCode::Char('ы') | KeyCode::Char('і') => {
            tokio::spawn(async_simple_command(app_arc, "/webhook/audio".to_string(), SimplePayload { action: "skip", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
        }
        KeyCode::Char('p') | KeyCode::Char('з') => {
            tokio::spawn(async_simple_command(app_arc, "/webhook/audio".to_string(), SimplePayload { action: "pause", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
        }
        KeyCode::Char('w') | KeyCode::Char('ц') => {
            tokio::spawn(async_simple_command(app_arc, "/webhook/audio".to_string(), SimplePayload { action: "stop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
        }
        KeyCode::Char('c') | KeyCode::Char('с') => {
            tokio::spawn(async_simple_command(app_arc, "/webhook/audio".to_string(), SimplePayload { action: "clear", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
        }
        KeyCode::Char('i') | KeyCode::Char('ш') => {
            app.view = View::AppInfo;
        }
        KeyCode::Char('d') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
            app.view = View::Debug;
        }
        KeyCode::Char(c) => {
            app.input_mode = InputMode::Editing;
            app.input.push(c);
        }
        _ => {}
    }
}

fn handle_lyrics_keys(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => app.view = View::Main,
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('о') => {
            app.lyrics_scroll = app.lyrics_scroll.saturating_add(1);
        },
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('л') => {
            app.lyrics_scroll = app.lyrics_scroll.saturating_sub(1);
        },
        _ => {}
    }
}

fn handle_settings_keys(app: &mut App, key: event::KeyEvent, app_arc: Arc<Mutex<App>>) {
    if app.is_settings_editing {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                app.is_settings_editing = false;
                save_app_settings(app);
                // If host changed, we might need reconnect
                if app.base_url != app.settings_input {
                    app.base_url = app.settings_input.clone();
                    app.needs_reconnect = true;
                    tokio::spawn(async_fetch_queue(app_arc));
                }
                if let Ok(offset) = app.offset_input.parse::<i64>() {
                    app.visualizer_offset = offset;
                }
            }
            KeyCode::Char(c) => {
                match app.settings_field {
                    SettingsField::Host => { app.settings_input.push(c); }
                    SettingsField::Offset => { 
                        if c.is_ascii_digit() || (c == '-' && app.offset_input.is_empty()) { 
                            app.offset_input.push(c); 
                        } 
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                match app.settings_field {
                    SettingsField::Host => { app.settings_input.pop(); }
                    SettingsField::Offset => { app.offset_input.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Enter => {
            match app.settings_field {
                SettingsField::Host | SettingsField::Offset => {
                    app.is_settings_editing = true;
                }
                _ => {
                    save_app_settings(app);
                    app.view = if app.token.is_none() { View::LoginRequired } else { View::Main };
                }
            }
        }
        KeyCode::Esc => {
            app.view = if app.token.is_none() { View::LoginRequired } else { View::Main };
        }
        KeyCode::Down | KeyCode::Tab => {
            app.settings_field = match app.settings_field {
                SettingsField::Host => SettingsField::Offset,
                SettingsField::Offset => SettingsField::Theme,
                SettingsField::Theme => SettingsField::VizStyle,
                SettingsField::VizStyle => SettingsField::Layout,
                SettingsField::Layout => SettingsField::Host,
            };
        }
        KeyCode::Up => {
            app.settings_field = match app.settings_field {
                SettingsField::Host => SettingsField::Layout,
                SettingsField::Offset => SettingsField::Host,
                SettingsField::Theme => SettingsField::Offset,
                SettingsField::VizStyle => SettingsField::Theme,
                SettingsField::Layout => SettingsField::VizStyle,
            };
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('д') => {
            match app.settings_field {
                SettingsField::Theme => {
                    app.theme = match app.theme.as_str() {
                        "Default" => "Midnight".to_string(),
                        "Midnight" => "Emerald".to_string(),
                        "Emerald" => "Ruby".to_string(),
                        "Ruby" => "Ocean".to_string(),
                        "Ocean" => "Synthwave".to_string(),
                        "Synthwave" => "Sepia".to_string(),
                        _ => "Default".to_string(),
                    };
                    save_app_settings(app);
                }
                SettingsField::VizStyle => {
                    app.viz_style = match app.viz_style.as_str() {
                        "Bars" => "Blocky".to_string(),
                        "Blocky" => "Line".to_string(),
                        "Line" => "Wave".to_string(),
                        "Wave" => "Dots".to_string(),
                        _ => "Bars".to_string(),
                    };
                    save_app_settings(app);
                }
                SettingsField::Layout => {
                    app.layout = match app.layout.as_str() {
                        "Standard" => "Sidebar".to_string(),
                        "Sidebar" => "Studio".to_string(),
                        "Studio" => "Zen".to_string(),
                        "Zen" => "Standard".to_string(),
                        _ => "Standard".to_string(),
                    };
                    save_app_settings(app);
                }
                _ => {}
            }
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('р') => {
            match app.settings_field {
                SettingsField::Theme => {
                    app.theme = match app.theme.as_str() {
                        "Default" => "Sepia".to_string(),
                        "Midnight" => "Default".to_string(),
                        "Emerald" => "Midnight".to_string(),
                        "Ruby" => "Emerald".to_string(),
                        "Ocean" => "Ruby".to_string(),
                        "Synthwave" => "Ocean".to_string(),
                        "Sepia" => "Synthwave".to_string(),
                        _ => "Default".to_string(),
                    };
                    save_app_settings(app);
                }
                SettingsField::VizStyle => {
                    app.viz_style = match app.viz_style.as_str() {
                        "Bars" => "Dots".to_string(),
                        "Blocky" => "Bars".to_string(),
                        "Line" => "Blocky".to_string(),
                        "Wave" => "Line".to_string(),
                        "Dots" => "Wave".to_string(),
                        _ => "Bars".to_string(),
                    };
                    save_app_settings(app);
                }
                SettingsField::Layout => {
                    app.layout = match app.layout.as_str() {
                        "Standard" => "Zen".to_string(),
                        "Sidebar" => "Standard".to_string(),
                        "Studio" => "Sidebar".to_string(),
                        "Zen" => "Studio".to_string(),
                        _ => "Standard".to_string(),
                    };
                    save_app_settings(app);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn save_app_settings(app: &App) {
    let settings = api::Settings { 
        base_url: app.settings_input.clone(),
        visualizer_offset: app.offset_input.parse().unwrap_or(app.visualizer_offset),
        theme: app.theme.clone(),
        visualizer_style: app.viz_style.clone(),
        layout: app.layout.clone(),
    };
    let _ = api::save_settings(&settings);
}

fn handle_debug_keys(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::Char('s') | KeyCode::Char('ы') => app.save_spectrogram(),
        KeyCode::Esc | KeyCode::Backspace => {
            app.view = if app.token.is_none() { View::LoginRequired } else { View::Main };
        }
        _ => {}
    }
}

fn handle_menu_keys(app: &mut App, key: event::KeyEvent, app_arc: Arc<Mutex<App>>) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Tab => { app.view = View::Main; }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('о') => {
            let i = match app.menu_state.selected() {
                Some(i) => if i >= app.menu_items.len() - 1 { 0 } else { i + 1 },
                None => 0,
            };
            app.menu_state.select(Some(i));
        }
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('л') => {
            let i = match app.menu_state.selected() {
                Some(i) => if i == 0 { app.menu_items.len() - 1 } else { i - 1 },
                None => 0,
            };
            app.menu_state.select(Some(i));
        }
        KeyCode::Enter => {
            if let Some(idx) = app.menu_state.selected() {
                let item = app.menu_items[idx].trim();
                if item.contains("Skip") { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "skip", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                else if item.contains("Pause/Resume") { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "pause", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                else if item.contains("Stop") { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "stop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                else if item.contains("Shuffle") { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "shuffle", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                else if item.contains("Clear Queue") { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "clear", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                else if item.contains("Loop Track") { app.loop_mode = "track".to_string(); tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: "track".to_string() })); }
                else if item.contains("Loop Queue") { app.loop_mode = "queue".to_string(); tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: "queue".to_string() })); }
                else if item.contains("Loop Off") { app.loop_mode = "off".to_string(); tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: "off".to_string() })); }
                else if item.contains("24/7 Mode") { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), TwentyFourSevenPayload { action: "247", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), enabled: None })); }
                else if item.contains("Filters...") { app.view = View::FilterMenu; }
                else if item.contains("Lyrics") { tokio::spawn(async_fetch_lyrics(app_arc.clone())); }
                else if item.contains("Play Turip") { tokio::spawn(async_play_track(app_arc.clone(), "https://open.spotify.com/track/2RQWB4Asy1rjZL4IUcJ7kn".to_string())); }
                else if item.contains("Auth") { app.view = View::AuthMenu; }
                else if item.contains("Settings") { 
                    app.settings_input = app.base_url.clone();
                    app.view = View::Settings; 
                }
                else if item.contains("Exit TUI") { return Ok(true); }

                if !item.contains("Filters...") && !item.contains("Lyrics") && !item.contains("Auth") && !item.contains("Settings") {
                    app.view = View::Main;
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_filter_menu_keys(app: &mut App, key: event::KeyEvent, app_arc: Arc<Mutex<App>>) {
    match key.code {
        KeyCode::Esc => app.view = View::Main,
        KeyCode::Backspace => app.view = View::Menu,
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('о') => {
            let i = match app.filter_state.selected() {
                Some(i) => if i >= app.filter_items.len() - 1 { 0 } else { i + 1 },
                None => 0,
            };
            app.filter_state.select(Some(i));
        }
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('л') => {
            let i = match app.filter_state.selected() {
                Some(i) => if i == 0 { app.filter_items.len() - 1 } else { i - 1 },
                None => 0,
            };
            app.filter_state.select(Some(i));
        }
        KeyCode::Enter => {
            if let Some(idx) = app.filter_state.selected() {
                let style = app.filter_items[idx];
                let filters = get_filters_for_style(style);
                let payload = FilterPayload {
                    action: "filter",
                    guild_id: app.guild_id.clone(),
                    user_id: app.user_id.clone(),
                    filters,
                };
                tokio::spawn(async_simple_command(app_arc, "/webhook/audio".to_string(), payload));
                app.view = View::Main;
            }
        }
        _ => {}
    }
}

fn handle_auth_menu_keys(app: &mut App, key: event::KeyEvent, app_arc: Arc<Mutex<App>>) {
    match key.code {
        KeyCode::Esc | KeyCode::Tab => app.view = View::Main,
        KeyCode::Backspace => app.view = View::Menu,
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('о') => {
            let i = match app.auth_menu_state.selected() {
                Some(i) => if i >= app.auth_menu_items.len() - 1 { 0 } else { i + 1 },
                None => 0,
            };
            app.auth_menu_state.select(Some(i));
        }
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('л') => {
            let i = match app.auth_menu_state.selected() {
                Some(i) => if i == 0 { app.auth_menu_items.len() - 1 } else { i - 1 },
                None => 0,
            };
            app.auth_menu_state.select(Some(i));
        }
        KeyCode::Enter => {
            if let Some(idx) = app.auth_menu_state.selected() {
                match app.auth_menu_items[idx] {
                    "Login" => { tokio::spawn(async_auth_login(app_arc)); }
                    "Signout" => { tokio::spawn(async_auth_signout(app_arc)); }
                    "Info" => {
                        if let Some(auth) = api::load_auth() {
                            let mut info = String::new();
                            if let Some(path) = api::config_file_path() {
                                info.push_str(&format!("Auth file: {}\n", path.display()));
                            }
                            info.push_str(&format!("User: {}\n", auth.username.unwrap_or_else(|| "Unknown".to_string())));
                            if let Some(avatar) = auth.avatar_url {
                                info.push_str(&format!("Avatar: {}\n", avatar));
                            }
                            let token_masked = if auth.token.len() > 8 {
                                format!("{}...{}", &auth.token[0..4], &auth.token[auth.token.len() - 4..])
                            } else {
                                auth.token
                            };
                            info.push_str(&format!("Token: {}", token_masked));
                            app.auth_info_text = Some(info);
                            app.view = View::AuthResult;
                        } else {
                            app.auth_info_text = Some("Not authenticated. Run Login.".to_string());
                            app.view = View::AuthResult;
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn get_filters_for_style(style: &str) -> AudioFilters {
    match style.to_lowercase().as_str() {
        "clear" => AudioFilters::default(),
        "bassboost" => AudioFilters {
            equalizer: Some(vec![
                EqualizerBand { band: 0, gain: 0.2 },
                EqualizerBand { band: 1, gain: 0.15 },
                EqualizerBand { band: 2, gain: 0.1 },
                EqualizerBand { band: 3, gain: 0.05 },
                EqualizerBand { band: 4, gain: 0.0 },
                EqualizerBand { band: 5, gain: -0.05 },
            ]),
            ..Default::default()
        },
        "soft" => AudioFilters {
            low_pass: Some(LowPassOptions { smoothing: Some(20.0) }),
            ..Default::default()
        },
        "nightcore" => AudioFilters {
            timescale: Some(TimescaleOptions { speed: Some(1.1), pitch: Some(1.1), rate: Some(1.0) }),
            ..Default::default()
        },
        "vaporwave" => AudioFilters {
            timescale: Some(TimescaleOptions { speed: Some(0.85), pitch: Some(0.8), rate: Some(1.0) }),
            ..Default::default()
        },
        "8d" => AudioFilters {
            rotation: Some(RotationOptions { rotation_hz: Some(0.2) }),
            ..Default::default()
        },
        "tremolo" => AudioFilters {
            tremolo: Some(TremoloOptions { frequency: Some(2.0), depth: Some(0.5) }),
            ..Default::default()
        },
        "vibrato" => AudioFilters {
            vibrato: Some(VibratoOptions { frequency: Some(2.0), depth: Some(0.5) }),
            ..Default::default()
        },
        "karaoke" => AudioFilters {
            karaoke: Some(KaraokeOptions { level: Some(1.0), mono_level: Some(1.0), filter_band: Some(220.0), filter_width: Some(100.0) }),
            ..Default::default()
        },
        _ => AudioFilters::default(),
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let theme = get_theme(&app.theme);
    
    // Base background color for the entire UI
    f.render_widget(Block::default().bg(theme.bg), f.area());

    if app.view == View::LoginRequired {
        let area = f.area();
        f.render_widget(Clear, area);
        f.render_widget(Block::default().bg(theme.bg), area);
        
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(12), // Logo
                Constraint::Length(10), // Text
                Constraint::Min(1),
            ])
            .split(area);

        // Logo
        let art_text: Vec<Line> = ASCII_LOGO.iter().map(|s| Line::from(Span::styled(*s, Style::default().fg(theme.primary)))).collect();
        let art_paragraph = Paragraph::new(art_text)
            .alignment(Alignment::Center);
        f.render_widget(art_paragraph, chunks[1]);

        // Text
        let login_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(theme.border))
            .padding(ratatui::widgets::Padding::uniform(1));

        let text = if app.is_loading || (app.auth_info_text.is_some() && app.auth_info_text.as_deref() != Some("Initializing login...")) {
             let status = app.auth_info_text.clone().unwrap_or_else(|| "Authenticating...".to_string());
             vec![
                Line::from(Span::styled(" AUTHENTICATING ", Style::default().add_modifier(Modifier::BOLD).bg(Color::Yellow).fg(Color::Black))),
                Line::from(""),
                Line::from(status),
                Line::from(""),
                Line::from(Span::styled("Please wait while we connect to Discord...", Style::default().fg(theme.text_secondary))),
             ]
        } else {
             vec![
                Line::from(Span::styled(" LOGIN REQUIRED ", Style::default().add_modifier(Modifier::BOLD).bg(Color::Red).fg(Color::White))),
                Line::from(""),
                Line::from("To use Jorik CLI, you must log in with your Discord account."),
                Line::from("This allows us to access your voice channels and manage playback."),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled(" ENTER ", Style::default().bg(theme.primary).fg(Color::Black).add_modifier(Modifier::BOLD)),
                    Span::raw(" to Login"),
                ]),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled(" \\ ", Style::default().bg(theme.highlight).fg(Color::Black).add_modifier(Modifier::BOLD)),
                    Span::raw(" to Change Host"),
                ]),
            ]
        };
        
        let p = Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(login_block)
            .wrap(Wrap { trim: true });
        
        let text_area = centered_rect(60, 30, area);
        f.render_widget(Clear, text_area);
        f.render_widget(p, text_area);
        return;
    }

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    let tabs_area = main_layout[0];
    let top_section = main_layout[1];
    let status_bar_area = main_layout[2];

    // Render Tabs
    let tab_titles = vec![" [1] PLAYER ", " [2] LYRICS ", " [3] SETTINGS ", " [4] DEBUG "];
    let selected_tab = match app.view {
        View::Main | View::Menu | View::FilterMenu | View::AuthMenu | View::AuthResult => 0,
        View::Lyrics => 1,
        View::Settings => 2,
        View::Debug => 3,
        _ => 0,
    };

    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(theme.border)))
        .select(selected_tab)
        .style(Style::default().fg(theme.text_secondary))
        .highlight_style(Style::default().fg(theme.highlight).add_modifier(Modifier::BOLD))
        .divider(Span::styled(" | ", Style::default().fg(theme.border)));

    f.render_widget(tabs, tabs_area);

    match app.view {
        View::Lyrics => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title(format!(" Lyrics {} ", if app.is_loading { " ⏳ " } else { "" }))
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(theme.primary));
            
            let text = app.lyrics_text.as_deref().unwrap_or("Loading...");
            let p = Paragraph::new(text)
                .block(block)
                .wrap(Wrap { trim: false })
                .scroll((app.lyrics_scroll, 0));
                
            f.render_widget(p, top_section);
        }
        View::Settings => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title(" Settings ")
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(theme.primary));
            
            let f_field = app.settings_field;
            let is_ed = app.is_settings_editing;
            
            let h_s = |f| if f_field == f { 
                if is_ed { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }
                else { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) }
            } else { Style::default().fg(theme.text_secondary) };

            let h_l = |f, l| if f_field == f { 
                if is_ed { format!(" >> [EDITING] {}", l) }
                else { format!(" >> {}", l) }
            } else { format!("    {}", l) };

            let p = Paragraph::new(vec![
                Line::from("Configure your experience:"),
                Line::from(""),
                Line::from(vec![
                    Span::styled(h_l(SettingsField::Host, "Webhook Host: "), h_s(SettingsField::Host)),
                    Span::styled(&app.settings_input, h_s(SettingsField::Host)),
                ]),
                Line::from(vec![
                    Span::styled(h_l(SettingsField::Offset, "Visualizer Offset (ms): "), h_s(SettingsField::Offset)),
                    Span::styled(&app.offset_input, h_s(SettingsField::Offset)),
                ]),
                Line::from(vec![
                    Span::styled(h_l(SettingsField::Theme, "Color Theme: "), h_s(SettingsField::Theme)),
                    Span::styled(format!("< {} >", app.theme), h_s(SettingsField::Theme)),
                ]),
                Line::from(vec![
                    Span::styled(h_l(SettingsField::VizStyle, "Visualizer Style: "), h_s(SettingsField::VizStyle)),
                    Span::styled(format!("< {} >", app.viz_style), h_s(SettingsField::VizStyle)),
                ]),
                Line::from(vec![
                    Span::styled(h_l(SettingsField::Layout, "UI Layout: "), h_s(SettingsField::Layout)),
                    Span::styled(format!("< {} >", app.layout), h_s(SettingsField::Layout)),
                ]),
                Line::from(""),
                Line::from(if is_ed {
                    Span::styled("TYPE TO EDIT, ENTER TO FINISH", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("NAVIGATE WITH ARROWS/TAB, ENTER ON TEXT TO EDIT, ESC TO EXIT", Style::default().fg(theme.text_secondary))
                }),
            ])
            .block(block)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
                
            f.render_widget(p, top_section);

            // Show cursor when editing settings
            if is_ed {
                let cursor_y = match f_field {
                    SettingsField::Host => top_section.y + 3,
                    SettingsField::Offset => top_section.y + 4,
                    _ => 0,
                };
                let prefix_len = match f_field {
                    SettingsField::Host => 27, // " >> [EDITING] Webhook Host: "
                    SettingsField::Offset => 37, // " >> [EDITING] Visualizer Offset (ms): "
                    _ => 0,
                };
                let input_len = match f_field {
                    SettingsField::Host => app.settings_input.len(),
                    SettingsField::Offset => app.offset_input.len(),
                    _ => 0,
                };
                if cursor_y > 0 {
                    f.set_cursor_position((
                        top_section.x + 1 + prefix_len + input_len as u16,
                        cursor_y,
                    ));
                }
            }
        }
        View::Debug => {
            let ws_status = if app.ws_connected {
                Span::styled(" CONNECTED ", Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD))
            } else if app.ws_connecting {
                Span::styled(" CONNECTING... ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(" DISCONNECTED ", Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD))
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .title(vec![
                    Span::raw(" Debug Console "), 
                    ws_status,
                    Span::raw(" (Press 's' to Save Spectrogram) ")
                ])
                .title_alignment(Alignment::Left)
                .border_style(Style::default().fg(Color::Yellow));
            
            let log_lines: Vec<Line> = app.debug_logs.iter()
                .rev()
                .map(|l| Line::from(l.as_str()))
                .collect();
            
            let p = Paragraph::new(log_lines)
                .block(block)
                .wrap(Wrap { trim: false });
            
            f.render_widget(p, top_section);
        }
        _ => {
            render_player_ui(f, app, &theme, top_section);
        }
    }

    if app.input_mode == InputMode::Normal && app.view == View::Main {
        let keys = vec![
            ("ENTER", "SEARCH"),
            ("TAB", "MENU"),
            ("S", "SKIP"),
            ("W", "STOP"),
            ("L", "LOOP"),
            ("R", "RELOAD"),
            ("I", "INFO"),
            ("Q", "QUIT"),
        ];
        
        let mut spans = Vec::new();
        spans.push(Span::styled(" >> ", Style::default().fg(theme.primary)));
        spans.push(Span::styled("COMMANDS ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
        
        for (key, desc) in keys {
            spans.push(Span::styled(format!(" {} ", key), Style::default().fg(theme.highlight).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!("{} ", desc), Style::default().fg(theme.text_secondary)));
            spans.push(Span::styled("|", Style::default().fg(theme.border)));
        }

        let version = env!("CARGO_PKG_VERSION");
        if version.chars().any(|c| c.is_ascii_lowercase()) {
            spans.push(Span::raw("   "));
            spans.push(Span::styled(" ! DEV UNSTABLE BUILD ! ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)));
        }

        let p = Paragraph::new(Line::from(spans))
            .style(Style::default().bg(theme.bg))
            .alignment(Alignment::Left)
            .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(theme.border)));
            
        f.render_widget(p, status_bar_area);
    }

    if app.input_mode == InputMode::Editing {
        let area = centered_rect(60, 20, f.area());
        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(format!(" Play / Search {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme.highlight));
        
        let p = Paragraph::new(app.input.as_str())
            .block(input_block)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);

        // Show cursor in Search popup
        f.set_cursor_position((
            area.x + 1 + app.input.len() as u16,
            area.y + 1,
        ));
    }

    if app.view == View::Menu {
        let area = centered_rect(40, 50, f.area());
        
        // Shadow
        let shadow_area = Rect { x: area.x + 1, y: area.y + 1, width: area.width, height: area.height };
        if shadow_area.right() < f.area().right() && shadow_area.bottom() < f.area().bottom() {
            f.render_widget(Block::default().bg(Color::Rgb(10, 10, 20)), shadow_area);
        }

        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let menu_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(format!(" Menu {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme.primary));
        
        let items: Vec<ListItem> = app.menu_items
            .iter()
            .map(|i| ListItem::new(format!("  {}  ", *i)))
            .collect();
            
        let list = List::new(items)
            .block(menu_block)
            .highlight_style(Style::default().bg(theme.primary).fg(Color::Black).add_modifier(Modifier::BOLD))
            .highlight_symbol(" >> ");
            
        f.render_stateful_widget(list, area, &mut app.menu_state);
    }

    if app.view == View::FilterMenu {
        let area = centered_rect(40, 50, f.area());
        
        // Shadow
        let shadow_area = Rect { x: area.x + 1, y: area.y + 1, width: area.width, height: area.height };
        if shadow_area.right() < f.area().right() && shadow_area.bottom() < f.area().bottom() {
            f.render_widget(Block::default().bg(Color::Rgb(10, 10, 20)), shadow_area);
        }

        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let menu_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(format!(" Select Filter {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme.primary));
        
        let items: Vec<ListItem> = app.filter_items
            .iter()
            .map(|i| ListItem::new(format!("  {}  ", *i)))
            .collect();
            
        let list = List::new(items)
            .block(menu_block)
            .highlight_style(Style::default().bg(theme.primary).fg(Color::Black).add_modifier(Modifier::BOLD))
            .highlight_symbol(" >> ");
            
        f.render_stateful_widget(list, area, &mut app.filter_state);
    }

    if app.view == View::AuthMenu {
        let area = centered_rect(40, 40, f.area());
        
        // Shadow
        let shadow_area = Rect { x: area.x + 1, y: area.y + 1, width: area.width, height: area.height };
        if shadow_area.right() < f.area().right() && shadow_area.bottom() < f.area().bottom() {
            f.render_widget(Block::default().bg(Color::Rgb(10, 10, 20)), shadow_area);
        }

        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let menu_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(format!(" Auth {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme.primary));
        
        let items: Vec<ListItem> = app.auth_menu_items
            .iter()
            .map(|i| ListItem::new(format!("  {}  ", *i)))
            .collect();
            
        let list = List::new(items)
            .block(menu_block)
            .highlight_style(Style::default().bg(theme.primary).fg(Color::Black).add_modifier(Modifier::BOLD))
            .highlight_symbol(" >> ");
            
        f.render_stateful_widget(list, area, &mut app.auth_menu_state);
    }

    if app.view == View::AuthResult {
        let area = centered_rect(60, 40, f.area());
        f.render_widget(Clear, area);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(" Auth Info ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme.primary));
        
        let text = app.auth_info_text.as_deref().unwrap_or("No data.");
        let p = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: true });
            
        f.render_widget(p, area);
    }

    if app.view == View::AppInfo {
        let area = centered_rect(60, 40, f.area());
        f.render_widget(Clear, area);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(" Build Compatibility Info ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(theme.highlight));
        
        let text = vec![
            Line::from(Span::styled("BUILD COMPATIBILITY", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))),
            Line::from(""),
            Line::from("This version of Jorik CLI is intended for use with"),
            Line::from(vec![
                Span::raw("the "),
                Span::styled("INTERNAL DEV VERSION", Style::default().fg(theme.highlight).add_modifier(Modifier::BOLD)),
                Span::raw(" of Jorik bot."),
            ]),
            Line::from(""),
            Line::from("The production version will work, but with significantly"),
            Line::from("reduced functionality (limited real-time features)."),
            Line::from(""),
            Line::from(vec![
                Span::raw("Current Version: "),
                Span::styled(env!("CARGO_PKG_VERSION"), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(Span::styled("Press 'i' or Esc to close", Style::default().fg(theme.text_secondary))),
        ];

        let p = Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(block)
            .wrap(Wrap { trim: true });
            
        f.render_widget(p, area);
    }

    if let Some(msg) = &app.fatal_error {
        let area = centered_rect(60, 25, f.area());
        f.render_widget(Clear, area);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .title(" ⚠ Connection Error ")
            .title_alignment(Alignment::Center)
            .style(Style::default())
            .border_style(Style::default().fg(Color::Red));
        
        let p = Paragraph::new(msg.as_str())
            .block(block)
            .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
            
        f.render_widget(p, area);
    }
}

fn render_player_ui(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    match app.layout.as_str() {
        "Sidebar" => render_sidebar_layout(f, app, theme, area),
        "Studio" => render_studio_layout(f, app, theme, area),
        "Zen" => render_zen_layout(f, app, theme, area),
        _ => render_standard_layout(f, app, theme, area),
    }
}

fn render_standard_layout(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(area);

    let left_side = content_chunks[0];
    let spectrogram_area = content_chunks[1];

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Length(6),
            Constraint::Min(0),
        ])
        .split(left_side);

    render_logo(f, theme, left_chunks[0]);
    render_now_playing(f, app, theme, left_chunks[1]);
    render_queue(f, app, theme, left_chunks[2]);
    render_visualizer(f, app, theme, spectrogram_area);
}

fn render_sidebar_layout(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(70), // Bigger Viz
            Constraint::Percentage(30),
        ])
        .split(area);

    let main_side = chunks[0];
    let sidebar = chunks[1];

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(6),
        ])
        .split(main_side);

    let sidebar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Min(0),
        ])
        .split(sidebar);

    render_visualizer(f, app, theme, main_chunks[0]);
    render_now_playing(f, app, theme, main_chunks[1]);
    render_logo(f, theme, sidebar_chunks[0]);
    render_queue(f, app, theme, sidebar_chunks[1]);
}

fn render_studio_layout(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Min(0),
            Constraint::Length(8),
        ])
        .split(area);

    let top_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    render_logo(f, theme, top_row[0]);
    render_now_playing(f, app, theme, top_row[1]);
    render_visualizer(f, app, theme, chunks[1]);
    render_queue(f, app, theme, chunks[2]);
}

fn render_zen_layout(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(6),
        ])
        .split(area);

    render_visualizer(f, app, theme, chunks[0]);
    render_now_playing(f, app, theme, chunks[1]);
}

fn render_logo(f: &mut Frame, theme: &Theme, area: Rect) {
    let art_text: Vec<Line> = ASCII_LOGO.iter().map(|s| Line::from(Span::styled(*s, Style::default().fg(theme.primary)))).collect();
    let art_paragraph = Paragraph::new(art_text)
        .alignment(Alignment::Center)
        .block(Block::default());
    f.render_widget(art_paragraph, area);
}

fn render_now_playing(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let playing_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.border))
        .title(" Now Playing ")
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));

    if let Some(current) = &app.current_track {
        let (title, artist) = if let Some((t, a)) = current.split_once(" - ") {
            (t, a)
        } else {
            (current.as_str(), "Unknown Artist")
        };

        let play_info = vec![
            Line::from(vec![
                Span::styled(" > ", Style::default().fg(theme.primary)),
                Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("   by ", Style::default().fg(theme.text_secondary)),
                Span::styled(artist, Style::default().fg(theme.highlight)),
            ]),
        ];

        let p = Paragraph::new(play_info).block(playing_block.clone());
        f.render_widget(p, area);

        if app.duration_ms > 0 {
            let ratio = (app.elapsed_ms as f64 / app.duration_ms as f64).min(1.0);
            let time_str = format!(
                " {:02}:{:02} / {:02}:{:02} ",
                app.elapsed_ms / 60000,
                (app.elapsed_ms % 60000) / 1000,
                app.duration_ms / 60000,
                (app.duration_ms % 60000) / 1000,
            );

            let gauge = Gauge::default()
                .block(Block::default().padding(ratatui::widgets::Padding::horizontal(2)))
                .gauge_style(Style::default().fg(theme.primary).bg(Color::Rgb(30, 30, 40)))
                .ratio(ratio)
                .label(time_str)
                .use_unicode(true);
            
            let gauge_area = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(1), Constraint::Min(0)])
                .split(area)[1];
            
            f.render_widget(gauge, gauge_area);
        }
    } else {
        f.render_widget(Paragraph::new("Nothing is playing").block(playing_block).alignment(Alignment::Center), area);
    }
}

fn render_queue(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let loop_status = app.loop_mode.to_uppercase();
    let loading_indicator = if app.is_loading { " [L] " } else { " " };
    let title = format!(" Queue ({}){} ", loop_status, loading_indicator);
    
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.border))
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
        .title(title);

    if let Some(err) = &app.error_message {
        let p = Paragraph::new(format!("! {}", err))
            .style(Style::default().fg(Color::Red))
            .block(content_block)
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
    } else {
        let mut items = Vec::new();
        if !app.queue.is_empty() {
             for (i, track) in app.queue.iter().enumerate() {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:2}. ", i + 1), Style::default().fg(theme.primary)),
                    Span::styled(track, Style::default().fg(theme.text_secondary)),
                ])));
            }
        } else {
             items.push(ListItem::new(Span::styled("   Queue is empty", Style::default().fg(Color::DarkGray))));
        }

        let list = List::new(items).block(content_block);
        f.render_widget(list, area);
    }
}

fn render_visualizer(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let spec_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.border))
        .title(" Visualizer ")
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));

    if app.current_track.is_some() {
        let (b_w, b_g) = match app.viz_style.as_str() {
            "Blocky" => (area.width / 64, 0),
            "Line" => (1, 0),
            "Wave" => (1, 0),
            "Dots" => (1, 1),
            _ => (2, 1),
        };

        let num_bars = if app.viz_style == "Wave" || app.viz_style == "Dots" {
            (area.width as usize).min(128)
        } else {
            ((area.width / (b_w + b_g)) as usize).min(64)
        };

        let mut bar_items = Vec::with_capacity(num_bars);

        if num_bars > 0 {
            let start_bin = 3.0;
            let end_bin = 61.0;
            let bins_to_show = end_bin - start_bin;
            let bins_per_bar = bins_to_show / num_bars as f32;

            for j in 0..num_bars {
                let start_f = start_bin + j as f32 * bins_per_bar;
                let end_f = start_bin + (j + 1) as f32 * bins_per_bar;
                let mut sum = 0.0;
                let mut weight = 0.0;
                for i in 0..64 {
                    let overlap = ((i + 1) as f32).min(end_f) - (i as f32).max(start_f);
                    if overlap > 0.0 {
                        sum += app.smoothed_bars[i] * overlap;
                        weight += overlap;
                    }
                }
                bar_items.push((if weight > 0.0 { sum / weight } else { 0.0 }) as u64);
            }
        }

        let bars: Vec<Bar> = bar_items.iter().enumerate()
            .map(|(i, &v)| {
                let color = match app.viz_style.as_str() {
                    "Blocky" | "Wave" => {
                        if i < num_bars / 3 { theme.primary }
                        else if i < 2 * num_bars / 3 { theme.highlight }
                        else { Color::Rgb(200, 200, 255) }
                    },
                    "Line" => theme.highlight,
                    _ => { // Bars (Gradient)
                        if i < num_bars / 4 { theme.primary }
                        else if i < num_bars / 2 { theme.highlight }
                        else { Color::Rgb(200, 200, 255) }
                    }
                };

                let label = if app.viz_style == "Line" || app.viz_style == "Wave" { String::new() } else { format!("{:2}", v.min(99)) };

                Bar::default()
                    .value(v)
                    .label(Span::from(label))
                    .style(Style::default().fg(color))
                    .text_value(String::new())
            })
            .collect();
        
        let bar_group = BarGroup::default().bars(&bars);
        let spec_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(spec_block.inner(area));

        let barchart = BarChart::default()
            .data(bar_group)
            .bar_width(b_w.max(1))
            .bar_gap(b_g)
            .max(100) 
            .label_style(Style::default().fg(theme.text_secondary));
        
        f.render_widget(spec_block, area);
        f.render_widget(barchart, spec_chunks[0]);

        if app.viz_style != "Wave" && app.viz_style != "Dots" {
            let labels = ["40", "100", "500", "1k", "5k", "10k", "16k"];
            let mut label_spans = Vec::new();
            let total_w = spec_chunks[1].width as usize;
            if total_w > 10 {
                for (i, &l) in labels.iter().enumerate() {
                    let pos = (i as f32 / (labels.len() - 1) as f32 * (total_w - l.len()) as f32) as usize;
                    let current_len: usize = label_spans.iter().map(|s: &Span| s.content.len()).sum();
                    if pos > current_len { label_spans.push(Span::raw(" ".repeat(pos - current_len))); }
                    label_spans.push(Span::styled(l, Style::default().fg(theme.text_secondary)));
                }
                f.render_widget(Paragraph::new(Line::from(label_spans)), spec_chunks[1]);
            }
        }
    } else {
        f.render_widget(Paragraph::new("Idle (No Track)").block(spec_block).alignment(Alignment::Center), area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    let horiz_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1]);

    horiz_layout[1]
}
