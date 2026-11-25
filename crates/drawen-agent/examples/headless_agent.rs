//! A Drawen application with no window at all: an LLM agent observes and
//! drives it through a local JSON-lines endpoint. No renderer, no GPU, no
//! display server — the semantic tree is the only surface.
//!
//! Run it with `cargo run -p drawen-agent --example headless_agent`, then
//! connect any TCP client to the printed address and send one JSON command
//! per line, for example:
//!
//! ```json
//! {"op":"observe"}
//! {"op":"set_value","target":{"role":"text_field"},"value":"Grace"}
//! {"op":"toggle","target":"Subscribe"}
//! {"op":"click","target":"Save"}
//! ```
//!
//! Every reply carries a fresh snapshot. Stop the demo with Ctrl+C.

use std::io;
use std::time::Duration;

use drawen::{
    Box as DrawenBox, Button, Checkbox, Color, Column, Constraints, Size, Subscription, Text,
    TextField, UiRuntime, agent::AgentServer, render::TextStyle,
};

fn main() -> io::Result<()> {
    let mut root = DrawenBox::new();
    root.set_background(Some(Color::WHITE));
    let mut runtime = UiRuntime::new(root);
    let column = runtime
        .append_child(runtime.root(), Column::new())
        .expect("root exists");

    runtime
        .append_child(column, TextField::new())
        .expect("column exists");

    let checkbox = Checkbox::new();
    let toggled = checkbox.toggled().subscribe(|checked| {
        println!("subscribe: {checked}");
    });
    let checkbox = runtime
        .append_child(column, checkbox)
        .expect("column exists");
    runtime
        .append_child(
            checkbox,
            Text::styled("Subscribe", TextStyle::new(14.0, Color::BLACK)),
        )
        .expect("checkbox exists");

    let button = Button::new();
    let activated = button.activated().subscribe(|()| println!("saved"));
    let button = runtime.append_child(column, button).expect("column exists");
    runtime
        .append_child(
            button,
            Text::styled("Save", TextStyle::new(14.0, Color::BLACK)),
        )
        .expect("button exists");

    let size = Size::new(360.0, 240.0).expect("demo size is valid");
    runtime
        .layout(Constraints::tight(size).expect("demo constraints are valid"))
        .expect("layout succeeds");

    let mut server = AgentServer::bind(9870)?;
    println!("agent endpoint: {}", server.local_addr());
    println!("send one JSON command per line; Ctrl+C stops the demo");

    let _subscriptions: Vec<Subscription> = vec![toggled, activated];
    loop {
        // The UI thread is this loop: drain commands, apply them, repaint nothing.
        let _changed = server.drain(&mut runtime);
        std::thread::sleep(Duration::from_millis(10));
    }
}
