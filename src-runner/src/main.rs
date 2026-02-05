#![windows_subsystem = "windows"]

use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use std::env;

fn main() {
    // Get the executable name to display
    let exe_name = env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "Discord Quest Runner".to_string());
    
    // Use generic title to avoid detection (no Discord Quest Helper references)
    let title = format!("{} - Background Service", exe_name);

    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(winit::dpi::LogicalSize::new(400.0, 100.0))
        .build(&event_loop)
        .unwrap();
    
    window.set_minimized(true);

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window.id() => elwt.exit(),
            _ => (),
        }
    }).unwrap();
}
