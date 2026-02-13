#![windows_subsystem = "windows"]

use softbuffer::Surface;
use std::env;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// Simple 5x7 pixel font for the message
const CHAR_WIDTH: usize = 6;
const CHAR_HEIGHT: usize = 8;

// Commit hash embedded at compile time by build.rs
const COMMIT_HASH: &str = env!("RUNNER_COMMIT_HASH");

// Basic 5x7 font data
fn get_char_bitmap(c: char) -> [u8; 7] {
    match c {
        // Letters (uppercase)
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b00000],
        'B' => [0b11110, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110, 0b00000],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10001, 0b01110, 0b00000],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110, 0b00000],
        'E' => [0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111, 0b00000],
        'F' => [0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b00000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b01111, 0b00000],
        'H' => [0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b00000],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110, 0b00000],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100, 0b00000],
        'K' => [0b10001, 0b10010, 0b11100, 0b10010, 0b10001, 0b10001, 0b00000],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111, 0b00000],
        'M' => [0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b00000],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b00000],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b00000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101, 0b00000],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10010, 0b10001, 0b00000],
        'S' => [0b01111, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110, 0b00000],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00000],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b11011, 0b10001, 0b00000],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b00000],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00000],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111, 0b00000],
        // Letters (lowercase)
        'a' => [0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111, 0b00000],
        'b' => [0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b11110, 0b00000],
        'c' => [0b00000, 0b01110, 0b10000, 0b10000, 0b10000, 0b01110, 0b00000],
        'd' => [0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b01111, 0b00000],
        'e' => [0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110, 0b00000],
        'f' => [0b00110, 0b01001, 0b01000, 0b11110, 0b01000, 0b01000, 0b00000],
        'g' => [0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b01110, 0b00000],
        'h' => [0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b00000],
        'i' => [0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b01110, 0b00000],
        'j' => [0b00010, 0b00000, 0b00110, 0b00010, 0b10010, 0b01100, 0b00000],
        'k' => [0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b00000],
        'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110, 0b00000],
        'm' => [0b00000, 0b11010, 0b10101, 0b10101, 0b10001, 0b10001, 0b00000],
        'n' => [0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001, 0b00000],
        'o' => [0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000],
        'p' => [0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000, 0b00000],
        'q' => [0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001, 0b00000],
        'r' => [0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000, 0b00000],
        's' => [0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110, 0b00000],
        't' => [0b01000, 0b01000, 0b11110, 0b01000, 0b01000, 0b00110, 0b00000],
        'u' => [0b00000, 0b10001, 0b10001, 0b10001, 0b10001, 0b01111, 0b00000],
        'v' => [0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00000],
        'w' => [0b00000, 0b10001, 0b10001, 0b10101, 0b11011, 0b10001, 0b00000],
        'x' => [0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b00000],
        'y' => [0b00000, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110, 0b00000],
        'z' => [0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111, 0b00000],
        // Digits
        '0' => [0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b01110, 0b00000],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110, 0b00000],
        '2' => [0b01110, 0b10001, 0b00010, 0b00100, 0b01000, 0b11111, 0b00000],
        '3' => [0b01110, 0b10001, 0b00110, 0b00001, 0b10001, 0b01110, 0b00000],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00000],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b10001, 0b01110, 0b00000],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110, 0b00000],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b00000],
        '8' => [0b01110, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110, 0b00000],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110, 0b00000],
        // Punctuation
        ':' => [0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000],
        ')' => [0b01000, 0b00100, 0b00100, 0b00100, 0b00100, 0b01000, 0b00000],
        '(' => [0b00010, 0b00100, 0b00100, 0b00100, 0b00100, 0b00010, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00000],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000],
        ' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        _ =>   [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
    }
}

fn draw_char(buffer: &mut [u32], width: usize, x: usize, y: usize, c: char, color: u32, scale: usize) {
    let bitmap = get_char_bitmap(c);
    for (row, &bits) in bitmap.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 1 {
                // Draw scaled pixel
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row * scale + sy;
                        if px < width && py < buffer.len() / width {
                            buffer[py * width + px] = color;
                        }
                    }
                }
            }
        }
    }
}

fn draw_text(buffer: &mut [u32], width: usize, x: usize, y: usize, text: &str, color: u32, scale: usize) {
    let char_width = CHAR_WIDTH * scale;

    for (i, c) in text.chars().enumerate() {
        draw_char(buffer, width, x + i * char_width, y, c, color, scale);
    }
}

/// Draw multiple lines of text, centered horizontally and vertically as a block
fn draw_text_block(buffer: &mut [u32], width: usize, height: usize, lines: &[(&str, u32, usize)]) {
    let line_spacing = 4; // pixels between lines
    // Calculate total block height
    let total_height: usize = lines.iter()
        .map(|(_, _, scale)| CHAR_HEIGHT * scale)
        .sum::<usize>()
        + line_spacing * lines.len().saturating_sub(1);

    let mut y = (height.saturating_sub(total_height)) / 2;
    for &(text, color, scale) in lines {
        let char_width = CHAR_WIDTH * scale;
        let text_width = text.len() * char_width;
        let x = (width.saturating_sub(text_width)) / 2;
        draw_text(buffer, width, x, y, text, color, scale);
        y += CHAR_HEIGHT * scale + line_spacing;
    }
}

fn main() {
    let exe_name = env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "Runner".to_string());

    // Build the version line: "Version: abc1234"
    let version_line = format!("Version: {}", COMMIT_HASH);

    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(
        WindowBuilder::new()
            .with_title(&exe_name)
            .with_inner_size(winit::dpi::LogicalSize::new(400.0, 120.0))
            .build(&event_loop)
            .unwrap(),
    );

    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = Surface::new(&context, window.clone()).unwrap();

    window.set_minimized(true);

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);

            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    window_id,
                } if window_id == window.id() => elwt.exit(),

                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    window_id,
                } if window_id == window.id() => {
                    let size = window.inner_size();
                    let width = size.width as usize;
                    let height = size.height as usize;

                    if width > 0 && height > 0 {
                        surface
                            .resize(
                                NonZeroU32::new(size.width).unwrap(),
                                NonZeroU32::new(size.height).unwrap(),
                            )
                            .unwrap();

                        let mut buffer = surface.buffer_mut().unwrap();

                        buffer.fill(0);

                        // Line 1: "Peace and Love :)" in white, scale 3
                        // Line 2: "Version: {hash}" in gray, scale 2
                        draw_text_block(&mut buffer, width, height, &[
                            ("Peace and Love :)", 0x00FFFFFF, 3),
                            (&version_line, 0x00888888, 2),
                        ]);

                        buffer.present().unwrap();
                    }
                }

                Event::WindowEvent {
                    event: WindowEvent::Resized(size),
                    window_id,
                } if window_id == window.id() => {
                    window.request_redraw();
                }

                Event::NewEvents(winit::event::StartCause::Init) => {
                    window.request_redraw();
                }

                _ => (),
            }
        })
        .unwrap();
}
