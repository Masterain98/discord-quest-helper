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

// Basic 5x7 font data for "Peace and Love :)"
fn get_char_bitmap(c: char) -> [u8; 7] {
    match c {
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'e' => [0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110, 0b00000],
        'a' => [0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111, 0b00000],
        'c' => [0b00000, 0b01110, 0b10000, 0b10000, 0b10000, 0b01110, 0b00000],
        'n' => [0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001, 0b00000],
        'd' => [0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b01111, 0b00000],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111, 0b00000],
        'o' => [0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000],
        'v' => [0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00000],
        ':' => [0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000],
        ')' => [0b01000, 0b00100, 0b00100, 0b00100, 0b00100, 0b01000, 0b00000],
        ' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        _ => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
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

fn draw_text(buffer: &mut [u32], width: usize, height: usize, text: &str, color: u32, scale: usize) {
    let char_width = CHAR_WIDTH * scale;
    let char_height = CHAR_HEIGHT * scale;
    let text_width = text.len() * char_width;
    
    let start_x = (width.saturating_sub(text_width)) / 2;
    let start_y = (height.saturating_sub(char_height)) / 2;

    for (i, c) in text.chars().enumerate() {
        draw_char(buffer, width, start_x + i * char_width, start_y, c, color, scale);
    }
}

fn main() {
    let exe_name = env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "Runner".to_string());

    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(
        WindowBuilder::new()
            .with_title(&exe_name)
            .with_inner_size(winit::dpi::LogicalSize::new(400.0, 100.0))
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

                        // Use 0x00FFFFFF (RGB White) to avoid alpha confusion
                        draw_text(&mut buffer, width, height, "Peace and Love :)", 0x00FFFFFF, 3);

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
