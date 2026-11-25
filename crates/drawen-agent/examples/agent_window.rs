//! A native Drawen window an LLM agent can also drive.
//!
//! The window behaves exactly like `minimal_window`; in addition an
//! `AgentServer` listens on loopback so an external model process can observe
//! the semantic tree and click, type, and press keys through the same input
//! paths a human uses. Commands arrive on a worker thread, cross to the UI
//! thread through the dispatcher, and are drained in `RedrawRequested` — the
//! canonical `background_tasks` pattern.
//!
//! Run it with `cargo run -p drawen-agent --example agent_window`, then send
//! JSON lines to `127.0.0.1:9870`, for example `{"op":"click","target":"Click me"}`.

use std::time::Duration;

use drawen::{
    Box as DrawenBox, Button, Color, Constraints, Size, Subscription, Text, UiRuntime,
    agent::AgentServer,
    platform::{
        WindowAction, WindowApplication, WindowEvent, WindowOptions, WindowPresentation,
        WindowWaker,
    },
    render::{DisplayList, PaintContext, TextStyle},
};

fn main() -> Result<(), drawen_platform_winit::RunError> {
    drawen_platform_winit::run(AgentWindow::new())
}

struct AgentWindow {
    runtime: UiRuntime,
    size: Size,
    server: AgentServer,
    _clicks: Subscription,
}

impl AgentWindow {
    fn new() -> Self {
        let mut root = DrawenBox::new();
        root.set_background(Some(Color::WHITE));
        let size = Size::new(480.0, 280.0).expect("window size is valid");
        let mut runtime = UiRuntime::new(root);

        let button = Button::new();
        let clicks = button.activated().subscribe(|()| println!("Clicked"));
        let button = runtime
            .append_child(runtime.root(), button)
            .expect("root exists");
        runtime
            .append_child(
                button,
                Text::styled("Click me", TextStyle::new(16.0, Color::BLACK)),
            )
            .expect("button exists");
        runtime
            .layout(Constraints::tight(size).expect("window constraints are valid"))
            .expect("layout succeeds");

        let server = AgentServer::bind(9870).expect("the loopback port binds");
        println!("agent endpoint: {}", server.local_addr());

        Self {
            runtime,
            size,
            server,
            _clicks: clicks,
        }
    }

    fn layout(&mut self, size: Size) {
        self.size = size;
        if let Ok(constraints) = Constraints::tight(size) {
            let _ = self.runtime.layout(constraints);
        }
    }
}

impl WindowApplication for AgentWindow {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::new("Drawen agent window", self.size)
    }

    fn attach_waker(&mut self, waker: WindowWaker) {
        self.server.attach_waker(move || waker.wake());
    }

    fn window_event(&mut self, event: WindowEvent) -> WindowAction {
        match event {
            WindowEvent::CloseRequested => WindowAction::Exit,
            WindowEvent::Resized(size) => {
                self.layout(size);
                WindowAction::RequestRedraw
            }
            WindowEvent::RedrawRequested => {
                if self.server.drain(&mut self.runtime) {
                    self.runtime.invalidate_paint();
                    return WindowAction::RequestRedraw;
                }
                WindowAction::None
            }
            WindowEvent::Input(event) => {
                let _ = self.runtime.dispatch_event(&event);
                if self.runtime.take_redraw_request() {
                    WindowAction::RequestRedraw
                } else {
                    WindowAction::None
                }
            }
            _ => WindowAction::None,
        }
    }

    fn presentation_changed(&mut self, presentation: WindowPresentation) -> WindowAction {
        self.runtime.set_presented(presentation.is_presented());
        WindowAction::None
    }

    fn redraw(&mut self) -> DisplayList {
        let mut display_list = DisplayList::new();
        let _ = self
            .runtime
            .paint(&mut PaintContext::new(&mut display_list));
        display_list
    }

    fn tick(&mut self, delta: Duration) -> bool {
        self.runtime.tick(delta)
    }

    fn next_frame_in(&self) -> Option<Duration> {
        self.runtime.next_frame_in()
    }
}
