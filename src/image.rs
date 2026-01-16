//! Image / terminal protocol helpers
//!
//! This module contains all of the logic related to detecting terminal image
//! protocols (iTerm2, Kitty, Sixel), encoding the embedded `logo.png` into
//! the appropriate sequence, and best-effort printing of the logo when the
//! CLI is invoked with `-V` / `--version`.

use ::image::{DynamicImage, imageops::FilterType};
use anyhow::{Context, Result};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use colored::Colorize;
use icy_sixel::{EncodeOptions, sixel_encode};
use ratatui::layout::Rect;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use std::fmt::Write as FmtWrite;
use std::io::{self, Cursor, Write};
use terminal_size::{Height, Width, terminal_size};

static LOGO_PNG: &[u8] = include_bytes!("../installer/assets/logo.png");

/// Print enhanced version information including detected image protocols and whether the
/// embedded logo is present in the binary.
///
/// `show_protocols` controls whether the protocol detection block (iTerm2, Kitty, Sixel
/// and logo presence) is printed. This lets callers show only the version by default and
/// print protocol support when explicitly requested.
pub fn print_version_info(show_protocols: bool) {
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    // Try to render the embedded logo before printing textual version info.
    // The helper returns whether an image was printed; if not, fall back to ASCII.
    let image_printed = match try_print_logo() {
        Ok(p) => p,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Warning: could not render logo: {}", err);
            false
        }
    };

    // If no terminal graphics were printed, show the ASCII fallback above the version line.
    if !image_printed {
        crate::ascii::print_ascii_logo();
    }

    println!("{} {}", name, version);

    if !show_protocols {
        return;
    }

    // Show protocol support details when explicitly requested.
    println!("Protocol support:");
    let iterm2 = detect_iterm2();
    let kitty = detect_kitty();
    let sixel = detect_sixel();

    println!(
        "  iTerm2: {}",
        if iterm2 { "Yes".green() } else { "No".red() }
    );
    println!(
        "  Kitty:  {}",
        if kitty { "Yes".green() } else { "No".red() }
    );
    println!(
        "  Sixel:  {}",
        if sixel { "Yes".green() } else { "No".red() }
    );

    println!(
        "Logo embedded: {} ({} bytes)",
        if LOGO_PNG.is_empty() {
            "No".red()
        } else {
            "Yes".green()
        },
        LOGO_PNG.len()
    );

    // If no protocols are supported, show ASCII fallback as well.
    if !iterm2 && !kitty && !sixel {
        println!("{}", "No supported graphic protocols detected.".yellow());
    }
}

/// Detect if running inside iTerm2.
///
/// Checks environment variables that iTerm2 sets: `TERM_PROGRAM == "iTerm.app"`
/// or presence of `ITERM_SESSION_ID`.
fn detect_iterm2() -> bool {
    std::env::var("TERM_PROGRAM")
        .map(|s| s == "iTerm.app")
        .unwrap_or(false)
        || std::env::var("ITERM_SESSION_ID").is_ok()
}

/// Detect if running inside the Kitty terminal emulator.
///
/// Checks `KITTY_WINDOW_ID`, `KITTY_PID`, or `TERM` containing "kitty".
fn detect_kitty() -> bool {
    std::env::var("KITTY_WINDOW_ID").is_ok()
        || std::env::var("KITTY_PID").is_ok()
        || std::env::var("TERM")
            .map(|s| s.to_lowercase().contains("kitty"))
            .unwrap_or(false)
}

/// Heuristic detection for Sixel support.
///
/// There's no reliable cross-terminal query for Sixel; this uses a set of heuristics:
/// - TERM contains "sixel"
/// - TERM matches some known terminal names that often have Sixel support (xterm, mlterm, kterm, rxvt)
/// - Presence of Windows Terminal environment variables (WT_SESSION / WT_PROFILE_ID)
fn detect_sixel() -> bool {
    // Explicit override (useful for testing or forcing behavior)
    if std::env::var("FORCE_SIXEL").is_ok() {
        return true;
    }

    // Windows Terminal commonly exposes WT_SESSION and/or WT_PROFILE_ID.
    // Treating their presence as evidence of Sixel support improves detection on Windows.
    if std::env::var("WT_SESSION").is_ok() || std::env::var("WT_PROFILE_ID").is_ok() {
        return true;
    }

    if let Ok(term) = std::env::var("TERM") {
        let term_l = term.to_lowercase();
        if term_l.contains("sixel") {
            return true;
        }
        // These terminals are commonly associated with sixel support; this is a heuristic
        let known = [
            "xterm", "mlterm", "kterm", "rxvt", "konsole", "sakura", "eterm",
        ];
        if known.iter().any(|t| term_l.contains(t)) {
            // Heuristic: many xterm-like terminals may support Sixel, but not all.
            return true;
        }
    }
    // No strong hint found
    false
}

/// Attempt to print the embedded logo using the best available protocol.
/// This is a best-effort helper: returns an error only if the encoding pipeline fails.
pub fn try_print_logo() -> Result<bool> {
    if LOGO_PNG.is_empty() {
        return Ok(false);
    }

    // Decode the embedded PNG
    let img_orig = ::image::load_from_memory(LOGO_PNG).context("decoding embedded logo")?;
    // Avoid upscaling: downscale only if terminal is smaller than the image.
    let img = maybe_downscale_image(&img_orig).context("downscaling logo")?;

    // Query terminal for font-size & capabilities. Fall back to safe defaults.
    // Avoid blocking interactive probes when stdout is not a TTY (for example in
    // non-interactive test runners or when output is being captured).
    if !atty::is(atty::Stream::Stdout) {
        // Non-TTY: pick a protocol based on env detection
        if detect_iterm2() {
            print_iterm2(&img)?;
            return Ok(true);
        }
        if detect_kitty() {
            print_kitty(&img)?;
            return Ok(true);
        }
        if detect_sixel() {
            print_sixel(&img)?;
            return Ok(true);
        }
        return Ok(false);
    }

    let picker = match Picker::from_query_stdio() {
        Ok(p) => p,
        Err(_) => {
            // If picker query fails, fall back to our previous detection path.
            // Keep prior behavior: prefer iTerm2, then Kitty, then Sixel.
            if detect_iterm2() {
                print_iterm2(&img)?;
                return Ok(true);
            }
            if detect_kitty() {
                print_kitty(&img)?;
                return Ok(true);
            }
            if detect_sixel() {
                print_sixel(&img)?;
                return Ok(true);
            }
            return Ok(false);
        }
    };

    // Get detected font size (character pixel size)
    let (char_w, char_h) = picker.font_size();

    // Convert image pixel size into character cell area (round-up)
    let img_cols = img.width().div_ceil(char_w as u32).max(1) as u16;
    let img_rows = img.height().div_ceil(char_h as u32).max(1) as u16;

    // Terminal column width (in characters)
    let term_cols = terminal_size().map(|(Width(w), _)| w).unwrap_or(80);
    let target_cols = img_cols.min(term_cols);

    // Build the target cell rectangle (we let the picker handle exact pixel mapping)
    let area = Rect::new(0, 0, target_cols, img_rows);

    // Debug output (if requested)
    if std::env::var("JORIK_IMAGE_DEBUG").is_ok() {
        eprintln!(
            "JORIK_IMAGE_DEBUG: img_px={}x{}, char_px={}x{}, img_cells={}x{}, term_cols={}, target_cols={}, area={:?}",
            img.width(),
            img.height(),
            char_w,
            char_h,
            img_cols,
            img_rows,
            term_cols,
            target_cols,
            area
        );
    }

    // Ask picker to produce a protocol (it will handle sizing properly)
    let proto = picker
        .new_protocol(img.clone(), area, Resize::Fit(None))
        .context("creating protocol for image rendering")?;

    // Debug info about chosen protocol
    if std::env::var("JORIK_IMAGE_DEBUG").is_ok() {
        match &proto {
            Protocol::ITerm2(it) => {
                eprintln!(
                    "JORIK_IMAGE_DEBUG: chosen protocol=iTerm2; encoded_len={}",
                    it.data.len()
                );
            }
            Protocol::Sixel(s) => {
                eprintln!(
                    "JORIK_IMAGE_DEBUG: chosen protocol=Sixel; encoded_len={}",
                    s.data.len()
                );
            }
            Protocol::Kitty(_) => {
                eprintln!(
                    "JORIK_IMAGE_DEBUG: chosen protocol=Kitty (fall back to internal encoder)"
                );
            }
            Protocol::Halfblocks(_) => {
                eprintln!("JORIK_IMAGE_DEBUG: chosen protocol=Halfblocks");
            }
        }
    }

    match proto {
        Protocol::ITerm2(it) => {
            // iTerm2 inline image (picker already encoded/resized as needed)
            print!("{}", it.data);
            io::stdout().flush().ok();
            Ok(true)
        }
        Protocol::Sixel(s) => {
            // Sixel payload (picker encoded and sized).
            print!("{}", s.data);
            io::stdout().flush().ok();
            Ok(true)
        }
        Protocol::Kitty(_) => {
            // Kitty variant chosen by picker — our internal Kitty string isn't public,
            // fall back to encoding the (possibly resized by picker) image directly.
            // This preserves correct sizing while still supporting Kitty.
            let seq = encode_kitty(&img)?;
            print!("{}", seq);
            io::stdout().flush().ok();
            Ok(true)
        }
        Protocol::Halfblocks(_) => {
            // Fallback: no graphics protocol; ASCII fallback will be handled elsewhere.
            Ok(false)
        }
    }
}

/// Print image using iTerm2 inline image escape sequence (base64 PNG).
fn encode_iterm2(img: &DynamicImage) -> Result<String> {
    // Re-encode the (possibly downscaled) image to PNG before sending to terminal.
    let mut png: Vec<u8> = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ::image::ImageFormat::Png)
        .context("encoding png for iterm2")?;
    let b64 = BASE64_STD.encode(&png);
    let seq = format!(
        "\x1b]1337;File=inline=1;size={};width={}px;height={}px;doNotMoveCursor=1:{}\x07",
        png.len(),
        img.width(),
        img.height(),
        b64
    );
    Ok(seq)
}

fn print_iterm2(img: &DynamicImage) -> Result<()> {
    let seq = encode_iterm2(img)?;
    print!("{seq}");
    io::stdout()
        .flush()
        .context("flushing stdout after writing iTerm2 image")?;
    Ok(())
}

/// Print image using the Kitty graphics protocol.
/// This function encodes the image as raw RGBA chunks and transmits them in base64 chunks.
fn encode_kitty(img: &DynamicImage) -> Result<String> {
    // Chunking size chosen to be reasonable for passthrough contexts like tmux.
    const CHUNK_SIZE: usize = 4096;

    let rgba = img.to_rgba8();
    let bytes = rgba.as_raw();
    let chunks: Vec<&[u8]> = bytes.chunks(CHUNK_SIZE).collect();
    let chunk_count = chunks.len();
    let (w, h) = (img.width(), img.height());

    let mut seq = String::new();
    for (i, chunk) in chunks.into_iter().enumerate() {
        let payload = BASE64_STD.encode(chunk);
        let more = if i + 1 < chunk_count { 1 } else { 0 };
        if i == 0 {
            // First chunk: include header with size and placement hints.
            // Use q=2 to indicate binary transfer; a=T and U=1 for virtual placement.
            write!(
                seq,
                "\x1b_Gq=2,i=1,a=T,U=1,f=32,t=d,s={},v={},m={};{}\x1b\\",
                w, h, more, payload
            )
            .map_err(|e| anyhow::anyhow!("formatting kitty chunk: {e}"))?;
        } else {
            write!(seq, "\x1b_Gq=2,m={};{}\x1b\\", more, payload)
                .map_err(|e| anyhow::anyhow!("formatting kitty chunk: {e}"))?;
        }
    }
    Ok(seq)
}

fn print_kitty(img: &DynamicImage) -> Result<()> {
    let seq = encode_kitty(img)?;
    print!("{seq}");
    io::stdout()
        .flush()
        .context("flushing stdout after writing Kitty image")?;
    Ok(())
}

/// Print image using the Sixel protocol (via the icy_sixel crate).
fn encode_sixel(img: &DynamicImage) -> Result<String> {
    let rgba = img.to_rgba8();
    let bytes = rgba.as_raw();
    let w = img.width() as usize;
    let h = img.height() as usize;

    let data = sixel_encode(bytes, w, h, &EncodeOptions::default())
        .map_err(|e| anyhow::anyhow!("sixel encoding error: {e}"))?;
    Ok(data)
}

fn print_sixel(img: &DynamicImage) -> Result<()> {
    let data = encode_sixel(img)?;
    print!("{data}");
    io::stdout()
        .flush()
        .context("flushing stdout after writing Sixel image")?;
    Ok(())
}

/// Downscale the image only if the terminal pixel area is smaller than the image.
/// This prevents upscaling: if the terminal is larger, we keep the image resolution.
fn maybe_downscale_image(img: &DynamicImage) -> Result<DynamicImage> {
    // Terminal size (cols, rows) in characters
    if let Some((Width(cols), Height(rows))) = terminal_size() {
        // Conservative defaults for character pixel size. These are heuristics;
        // they prevent upscaling in the common case.
        const CHAR_W: u32 = 8;
        const CHAR_H: u32 = 16;
        let term_px_w = (cols as u32).saturating_mul(CHAR_W);
        let term_px_h = (rows as u32).saturating_mul(CHAR_H);

        // If the image already fits within the terminal pixel area, do not upscale it.
        if img.width() <= term_px_w && img.height() <= term_px_h {
            if std::env::var("JORIK_IMAGE_DEBUG").is_ok() {
                eprintln!(
                    "JORIK_IMAGE_DEBUG: image fits terminal; img_px={}x{}, term_px={}x{} -> no downscale",
                    img.width(),
                    img.height(),
                    term_px_w,
                    term_px_h
                );
            }
            return Ok(img.clone());
        }

        // Downscale to fit terminal, preserving aspect ratio.
        let scale_w = term_px_w as f32 / img.width() as f32;
        let scale_h = term_px_h as f32 / img.height() as f32;
        let scale = scale_w.min(scale_h).min(1.0);
        let new_w = (img.width() as f32 * scale).max(1.0) as u32;
        let new_h = (img.height() as f32 * scale).max(1.0) as u32;

        if std::env::var("JORIK_IMAGE_DEBUG").is_ok() {
            eprintln!(
                "JORIK_IMAGE_DEBUG: downscaling image: img_px={}x{} -> {}x{} (term_px={}x{}, scale={:.3})",
                img.width(),
                img.height(),
                new_w,
                new_h,
                term_px_w,
                term_px_h,
                scale
            );
        }

        let resized = ::image::imageops::resize(img, new_w, new_h, FilterType::Lanczos3);
        return Ok(DynamicImage::ImageRgba8(resized));
    }

    // If terminal size isn't available, fall back to original image (no upscaling).
    if std::env::var("JORIK_IMAGE_DEBUG").is_ok() {
        eprintln!(
            "JORIK_IMAGE_DEBUG: terminal size unavailable; keeping image at original resolution {}x{}",
            img.width(),
            img.height()
        );
    }
    Ok(img.clone())
}
