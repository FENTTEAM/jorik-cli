use crate::api::{self, AudioFilters, EqualizerBand, FilterPayload, KaraokeOptions, LoopPayload, LowPassOptions, LyricsPayload, PlayPayload, QueuePayload, RotationOptions, SimplePayload, TimescaleOptions, TremoloOptions, TwentyFourSevenPayload, VibratoOptions};
use crate::ascii::ASCII_LOGO;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use reqwest::Client;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::{interval, timeout};
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};



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
}

impl App {
    fn new(
        client: Client,
        base_url: String,
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
            base_url,
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
                "Skip", "Pause/Resume", "Stop", "Shuffle", 
                "Clear Queue", "Loop Track", "Loop Queue", "Loop Off",
                "24/7 Mode Toggle", "Filters...", "Lyrics", "Play Turip",
                "Auth", "Exit TUI"
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
        }
    }

    fn parse_queue_response(&mut self, json: &Value) {
        if let Some(current) = json.get("current").and_then(|v| v.as_object()) {
            let title = current.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown");
            let author = current.get("author").and_then(|v| v.as_str()).unwrap_or("");
            self.current_track = Some(format!("{} - {}", title, author));
        } else {
            self.current_track = None;
        }

        self.queue.clear();
        if let Some(upcoming) = json.get("upcoming").and_then(|v| v.as_array()) {
            for item in upcoming {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown");
                let author = item.get("author").and_then(|v| v.as_str()).unwrap_or("");
                self.queue.push(format!("{} - {}", title, author));
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
    let (client, url, token, payload) = {
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
        (app.client.clone(), url, app.token.clone(), payload)
    };

    let mut req = client.post(&url).json(&payload);
    if let Some(bearer) = &token {
        req = req.bearer_auth(bearer);
    }

    let _ = req.send().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    async_fetch_queue(app_arc).await;
}

async fn async_fetch_lyrics(app_arc: Arc<Mutex<App>>) {
    let (client, url, token, payload) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        let payload = LyricsPayload {
            action: "lyrics".to_string(),
            guild_id: app.guild_id.clone(),
            user_id: app.user_id.clone(),
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
    let (client, url, token) = {
        let mut app = app_arc.lock().await;
        app.is_loading = true;
        let url = api::build_url(&app.base_url, &endpoint);
        (app.client.clone(), url, app.token.clone())
    };

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

pub async fn run(
    base_url: String,
    token: Option<String>,
    guild_id: Option<String>,
    user_id: Option<String>,
) -> Result<()> {
    let client = Client::builder()
        .user_agent("jorik-cli-tui")
        .timeout(Duration::from_secs(10))
        .build()?;

    let app = Arc::new(Mutex::new(App::new(client, base_url, token, guild_id, user_id)));
    
    // Initial fetch
    tokio::spawn(async_fetch_queue(app.clone()));

    let app_clone = app.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(300));
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
            terminal.draw(|f| ui(f, &mut *app))?;
        }

        if event::poll(Duration::from_millis(50))? {
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
                        match key.code {
                            KeyCode::Enter => {
                                let query = app.input.clone();
                                app.input.clear();
                                app.input_mode = InputMode::Normal;
                                tokio::spawn(async_play_track(app_arc.clone(), query));
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.input.clear();
                            }
                            KeyCode::Char(c) => {
                                app.input.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input.pop();
                            }
                            _ => {}
                        }
                    } else {
                        match app.view {
                            View::Menu => {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Tab => app.view = View::Main,
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
                                            let item = app.menu_items[idx];
                                            match item {
                                                "Skip" => { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "skip", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                                                "Pause/Resume" => { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "pause", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                                                "Stop" => { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "stop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                                                "Shuffle" => { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "shuffle", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                                                "Clear Queue" => { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "clear", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() })); }
                                                "Loop Track" => { app.loop_mode = "track".to_string(); tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: "track".to_string() })); }
                                                "Loop Queue" => { app.loop_mode = "queue".to_string(); tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: "queue".to_string() })); }
                                                "Loop Off" => { app.loop_mode = "off".to_string(); tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: "off".to_string() })); }
                                                "24/7 Mode Toggle" => { tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), TwentyFourSevenPayload { action: "247", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), enabled: None })); }
                                                "Filters..." => { app.view = View::FilterMenu; }
                                                "Lyrics" => { tokio::spawn(async_fetch_lyrics(app_arc.clone())); }
                                                "Play Turip" => { tokio::spawn(async_play_track(app_arc.clone(), "https://open.spotify.com/track/2RQWB4Asy1rjZL4IUcJ7kn".to_string())); }
                                                "Auth" => { app.view = View::AuthMenu; }
                                                "Exit TUI" => return Ok(()),
                                                _ => {}
                                            }
                                            if item != "Filters..." && item != "Lyrics" && item != "Auth" {
                                                app.view = View::Main;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            },
                            View::AuthMenu => {
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
                                                "Login" => {
                                                    tokio::spawn(async_auth_login(app_arc.clone()));
                                                }
                                                "Signout" => {
                                                    tokio::spawn(async_auth_signout(app_arc.clone()));
                                                }
                                                "Info" => {
                                                    if let Some(auth) = api::load_auth() {
                                                        let mut info = String::new();
                                                        if let Some(path) = api::config_file_path() {
                                                            info.push_str(&format!("Auth file: {}\n", path.display()));
                                                        }
                                                        info.push_str(&format!("User: {}\n", auth.username.unwrap_or_else(|| "Unknown".to_string())));
                                                        if let Some(avatar) = auth.avatar_url {
                                                            info.push_str(&format!("Avatar: {}\n", avatar));
                                                        } else {
                                                            info.push_str("Avatar: (none)\n");
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
                            },
                            View::AuthResult => {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace => app.view = View::AuthMenu,
                                    _ => {}
                                }
                            },
                            View::LoginRequired => {
                                match key.code {
                                    KeyCode::Enter => {
                                        tokio::spawn(async_auth_login(app_arc.clone()));
                                    }
                                    KeyCode::Char('q') | KeyCode::Char('й') => return Ok(()),
                                    _ => {}
                                }
                            },
                            View::FilterMenu => {
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
                                            tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), payload));
                                            app.view = View::Main;
                                        }
                                    }
                                    _ => {}
                                }
                            },
                            View::Lyrics => {
                                match key.code {
                                    KeyCode::Esc => app.view = View::Main,
                                    KeyCode::Backspace => app.view = View::Menu,
                                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('о') => {
                                        app.lyrics_scroll = app.lyrics_scroll.saturating_add(1);
                                    },
                                    KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('л') => {
                                        app.lyrics_scroll = app.lyrics_scroll.saturating_sub(1);
                                    },
                                    _ => {}
                                }
                            },
                            View::Main => {
                                match key.code {
                                    KeyCode::Char('q') | KeyCode::Char('й') => return Ok(()),
                                    KeyCode::Char('r') | KeyCode::Char('к') => {
                                        tokio::spawn(async_fetch_queue(app_arc.clone()));
                                    }
                                    KeyCode::Tab => {
                                        app.view = View::Menu;
                                    }
                                    KeyCode::Enter => {
                                        app.input_mode = InputMode::Editing;
                                    }
                                    KeyCode::Char('l') | KeyCode::Char('д') => {
                                        let new_mode = match app.loop_mode.as_str() {
                                            "off" => "track",
                                            "track" => "queue",
                                            "queue" => "off",
                                            _ => "off",
                                        };
                                        app.loop_mode = new_mode.to_string();
                                        tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), LoopPayload { action: "loop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone(), loop_mode: new_mode.to_string() }));
                                    }
                                    KeyCode::Char('s') | KeyCode::Char('ы') | KeyCode::Char('і') => {
                                        tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "skip", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
                                    }
                                    KeyCode::Char('w') | KeyCode::Char('ц') => {
                                        tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "stop", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
                                    }
                                    KeyCode::Char('c') | KeyCode::Char('с') => {
                                        tokio::spawn(async_simple_command(app_arc.clone(), "/webhook/audio".to_string(), SimplePayload { action: "clear", guild_id: app.guild_id.clone(), user_id: app.user_id.clone() }));
                                    }
                                    KeyCode::Char(c) => {
                                        app.input_mode = InputMode::Editing;
                                        app.input.push(c);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
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
    if app.view == View::LoginRequired {
        let area = f.area();
        f.render_widget(Clear, area);
        
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(12), // Logo
                Constraint::Length(2),  // Spacer
                Constraint::Length(8),  // Text
                Constraint::Min(1),
            ])
            .split(area);

        // Logo
        let art_text: Vec<Line> = ASCII_LOGO.iter().map(|s| Line::from(Span::styled(*s, Style::default().fg(JORIK_PURPLE)))).collect();
        let art_paragraph = Paragraph::new(art_text)
            .alignment(Alignment::Center);
        f.render_widget(art_paragraph, chunks[1]);

        // Text
        let text = if app.is_loading || (app.auth_info_text.is_some() && app.auth_info_text.as_deref() != Some("Initializing login...")) {
             let status = app.auth_info_text.clone().unwrap_or_else(|| "Authenticating...".to_string());
             vec![
                Line::from(Span::styled("Authenticating...", Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow))),
                Line::from(""),
                Line::from(status),
             ]
        } else {
             vec![
                Line::from(Span::styled("Authentication Required", Style::default().add_modifier(Modifier::BOLD).fg(Color::Red))),
                Line::from(""),
                Line::from("To use Jorik CLI, you must log in with your Discord account."),
                Line::from("This allows us to access your voice channels and manage playback."),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD).fg(JORIK_PURPLE)),
                    Span::raw(" to Login"),
                ]),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("q", Style::default().add_modifier(Modifier::BOLD).fg(JORIK_PURPLE)),
                    Span::raw(" to Quit"),
                ]),
            ]
        };
        
        let p = Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        f.render_widget(p, chunks[3]);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12), // ASCII Art height
            Constraint::Min(0),     // Queue content
            Constraint::Length(3),  // Input/Status
        ])
        .split(f.area());

    // 1. ASCII Art
    let art_text: Vec<Line> = ASCII_LOGO.iter().map(|s| Line::from(Span::styled(*s, Style::default().fg(JORIK_PURPLE)))).collect();
    let art_paragraph = Paragraph::new(art_text)
        .alignment(Alignment::Center)
        .block(Block::default());
    f.render_widget(art_paragraph, chunks[0]);

    // 2. Main Content (Queue or Error)
    let loop_status = app.loop_mode.to_uppercase();
    let loading_indicator = if app.is_loading { " ⏳ Loading... " } else { " " };
    let title = format!(" Queue (Loop: {}){} ", loop_status, loading_indicator);
    
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(JORIK_PURPLE))
        .title_style(Style::default().fg(JORIK_PURPLE).add_modifier(Modifier::BOLD))
        .title(title)
        .style(Style::default());

    if let Some(err) = &app.error_message {
        let p = Paragraph::new(format!("⚠ {}", err))
            .style(Style::default().fg(Color::Red))
            .block(content_block)
            .wrap(Wrap { trim: true });
        f.render_widget(p, chunks[1]);
    } else {
        let mut items = Vec::new();
        
        if let Some(current) = &app.current_track {
             items.push(ListItem::new(Line::from(vec![
                Span::styled(" NOW PLAYING ", Style::default().bg(JORIK_PURPLE).fg(Color::Black).add_modifier(Modifier::BOLD)),
             ])));
             items.push(ListItem::new(Line::from(vec![
                Span::styled("   ▶ ", Style::default().fg(JORIK_PURPLE)),
                Span::styled(current, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ])));
            items.push(ListItem::new(Span::raw("")));
        } else {
            items.push(ListItem::new(Span::styled("Nothing playing", Style::default().fg(Color::DarkGray))));
            items.push(ListItem::new(Span::raw("")));
        }
        
        if !app.queue.is_empty() {
             items.push(ListItem::new(Line::from(vec![
                Span::styled(" UP NEXT ", Style::default().fg(JORIK_PURPLE).add_modifier(Modifier::BOLD)),
             ])));
             for (i, track) in app.queue.iter().enumerate() {
                items.push(ListItem::new(format!("   {}. {}", i + 1, track)).style(Style::default().fg(Color::Gray)));
            }
        } else {
             items.push(ListItem::new(Span::styled("   Queue is empty", Style::default().fg(Color::DarkGray))));
        }

        let list = List::new(items)
            .block(content_block);
        f.render_widget(list, chunks[1]);
    }

    // 3. Status Bar / Hint
    if app.input_mode == InputMode::Normal && app.view == View::Main {
        let keys = vec![
            ("Type", "Search"),
            ("Enter", "Play"),
            ("Tab", "Menu"),
            ("s", "Skip"),
            ("w", "Stop"),
            ("c", "Clear"),
            ("l", "Loop"),
            ("r", "Refresh"),
            ("q", "Quit"),
        ];
        
        let mut spans = Vec::new();
        for (key, desc) in keys {
            spans.push(Span::styled(format!(" {} ", key), Style::default().bg(JORIK_PURPLE).fg(Color::Black).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled(format!(" {} ", desc), Style::default().fg(Color::Gray)));
            spans.push(Span::raw(" "));
        }

        let p = Paragraph::new(Line::from(spans))
            .style(Style::default())
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::TOP).border_type(BorderType::Double).border_style(Style::default().fg(Color::DarkGray)));
            
        f.render_widget(p, chunks[2]);
    }

    // Overlays

    // Input Box
    if app.input_mode == InputMode::Editing {
        let area = centered_rect(60, 20, f.area());
        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Play / Search {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(JORIK_HIGHLIGHT));
        
        let p = Paragraph::new(app.input.as_str())
            .block(input_block)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
    }

    // Menu Box
    if app.view == View::Menu {
        let area = centered_rect(40, 50, f.area());
        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let menu_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Menu {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(JORIK_PURPLE));
        
        let items: Vec<ListItem> = app.menu_items
            .iter()
            .map(|i| ListItem::new(format!("  {}  ", *i)))
            .collect();
            
        let list = List::new(items)
            .block(menu_block)
            .highlight_style(Style::default().bg(JORIK_PURPLE).fg(Color::White).add_modifier(Modifier::BOLD))
            .highlight_symbol(" ➤ ");
            
        f.render_stateful_widget(list, area, &mut app.menu_state);
    }

    // Filter Menu Box
    if app.view == View::FilterMenu {
        let area = centered_rect(40, 50, f.area());
        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let menu_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Select Filter {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(JORIK_PURPLE));
        
        let items: Vec<ListItem> = app.filter_items
            .iter()
            .map(|i| ListItem::new(format!("  {}  ", *i)))
            .collect();
            
        let list = List::new(items)
            .block(menu_block)
            .highlight_style(Style::default().bg(JORIK_PURPLE).fg(Color::White).add_modifier(Modifier::BOLD))
            .highlight_symbol(" ➤ ");
            
        f.render_stateful_widget(list, area, &mut app.filter_state);
    }

    // Auth Menu Box
    if app.view == View::AuthMenu {
        let area = centered_rect(40, 40, f.area());
        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let menu_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Auth {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(JORIK_PURPLE));
        
        let items: Vec<ListItem> = app.auth_menu_items
            .iter()
            .map(|i| ListItem::new(format!("  {}  ", *i)))
            .collect();
            
        let list = List::new(items)
            .block(menu_block)
            .highlight_style(Style::default().bg(JORIK_PURPLE).fg(Color::White).add_modifier(Modifier::BOLD))
            .highlight_symbol(" ➤ ");
            
        f.render_stateful_widget(list, area, &mut app.auth_menu_state);
    }

    // Auth Result/Info Box
    if app.view == View::AuthResult {
        let area = centered_rect(60, 40, f.area());
        f.render_widget(Clear, area);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Auth Info ")
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(JORIK_PURPLE));
        
        let text = app.auth_info_text.as_deref().unwrap_or("No data.");
        let p = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: true });
            
        f.render_widget(p, area);
    }

    // Lyrics Box
    if app.view == View::Lyrics {
        let area = centered_rect(70, 70, f.area());
        f.render_widget(Clear, area);
        
        let loading_text = if app.is_loading { " ⏳ " } else { "" };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" Lyrics {} ", loading_text))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(JORIK_PURPLE));
        
        let text = app.lyrics_text.as_deref().unwrap_or("Loading...");
        let p = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.lyrics_scroll, 0));
            
        f.render_widget(p, area);
    }

    // Fatal Error Overlay
    if let Some(msg) = &app.fatal_error {
        let area = centered_rect(60, 25, f.area());
        f.render_widget(Clear, area);
        
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
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