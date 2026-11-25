//! End-to-end test of the loopback JSON-lines bridge: a client thread speaks
//! the protocol while the test thread plays the UI thread and drains commands.

#![cfg(feature = "serve")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use std::{cell::Cell, rc::Rc};

use drawen::{
    Box as DrawenBox, Button, Color, Constraints, Size, Subscription, Text, UiRuntime,
    agent::{AgentServer, Reply},
    render::TextStyle,
};

#[test]
fn agent_process_observes_and_clicks_over_tcp() {
    let mut runtime = UiRuntime::new(DrawenBox::new());
    let clicks = Rc::new(Cell::new(0_u32));
    let button = Button::new();
    let subscription: Subscription = button.activated().subscribe({
        let clicks = Rc::clone(&clicks);
        move |()| clicks.set(clicks.get() + 1)
    });
    let button = runtime
        .append_child(runtime.root(), button)
        .expect("root exists");
    runtime
        .append_child(
            button,
            Text::styled("Save", TextStyle::new(14.0, Color::BLACK)),
        )
        .expect("button exists");
    runtime
        .layout(Constraints::tight(Size::new(320.0, 200.0).expect("valid size")).expect("valid"))
        .expect("layout succeeds");

    let mut server = AgentServer::bind(0).expect("loopback binds");
    let address = server.local_addr();
    let done = Arc::new(AtomicBool::new(false));
    let client_done = Arc::clone(&done);
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).expect("server accepts");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));

        writeln!(stream, "{{\"op\":\"observe\"}}").expect("write observe");
        stream.flush().expect("flush");
        let mut line = String::new();
        reader.read_line(&mut line).expect("observe reply");
        let reply: Reply = serde_json::from_str(&line).expect("reply parses");
        assert!(reply.ok, "{:?}", reply.error);
        let snapshot = reply.snapshot.expect("observe carries a snapshot");
        assert!(
            snapshot
                .nodes
                .iter()
                .any(|node| node.role == "button" && node.label.as_deref() == Some("Save")),
            "snapshot lists the button: {}",
            snapshot.to_text()
        );

        writeln!(stream, "{{\"op\":\"click\",\"target\":\"Save\"}}").expect("write click");
        stream.flush().expect("flush");
        line.clear();
        reader.read_line(&mut line).expect("click reply");
        let reply: Reply = serde_json::from_str(&line).expect("reply parses");
        assert!(reply.ok && reply.handled, "{:?}", reply.error);

        writeln!(stream, "{{\"op\":\"click\",\"target\":\"Missing\"}}").expect("write bad click");
        stream.flush().expect("flush");
        line.clear();
        reader.read_line(&mut line).expect("error reply");
        let reply: Reply = serde_json::from_str(&line).expect("reply parses");
        assert!(!reply.ok);
        assert!(reply.error.expect("error").contains("no widget matches"));

        writeln!(stream, "not json").expect("write garbage");
        stream.flush().expect("flush");
        line.clear();
        reader.read_line(&mut line).expect("parse-error reply");
        let reply: Reply = serde_json::from_str(&line).expect("reply parses");
        assert!(!reply.ok);
        assert!(reply.error.expect("error").contains("invalid command"));

        client_done.store(true, Ordering::SeqCst);
    });

    // The test thread is the UI thread: poll the drain like a headless loop.
    let mut turns = 0;
    while !done.load(Ordering::SeqCst) {
        server.drain(&mut runtime);
        std::thread::sleep(Duration::from_millis(2));
        turns += 1;
        assert!(turns < 5000, "the client did not finish within 10 seconds");
    }
    client.join().expect("client thread does not panic");
    assert_eq!(clicks.get(), 1);
    drop(server);
    drop(subscription);
}
