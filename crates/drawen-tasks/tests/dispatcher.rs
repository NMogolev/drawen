//! Integration tests for the worker-to-UI dispatcher.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use drawen_tasks::{TaskPool, UiDispatcher, UiSendError, UiSender};

fn assert_send_sync<T: Send + Sync>() {}
fn assert_send<T: Send>() {}

#[test]
fn worker_messages_are_delivered_and_wake_the_ui() {
    assert_send_sync::<UiSender<u32>>();
    assert_send::<UiDispatcher<u32>>();

    let wakes = Arc::new(AtomicUsize::new(0));
    let (dispatcher, sender) = {
        let wakes = Arc::clone(&wakes);
        UiDispatcher::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
        })
    };

    sender.send(1).expect("dispatcher is alive");
    sender.send(2).expect("dispatcher is alive");

    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert_eq!(dispatcher.len(), 2);
    assert_eq!(dispatcher.drain(), vec![1, 2]);
    assert!(dispatcher.is_empty());

    sender.send(3).expect("dispatcher is alive");
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(dispatcher.drain(), vec![3]);
}

#[test]
fn a_burst_of_sends_coalesces_into_one_wake() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let (dispatcher, sender) = {
        let wakes = Arc::clone(&wakes);
        UiDispatcher::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
        })
    };

    for value in 0..1000 {
        sender.send(value).expect("dispatcher is alive");
    }

    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert_eq!(dispatcher.drain().len(), 1000);
    assert!(dispatcher.is_empty());
}

#[test]
fn receiving_rearms_the_wake_while_values_remain() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let (dispatcher, sender) = {
        let wakes = Arc::clone(&wakes);
        UiDispatcher::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
        })
    };

    sender.send(1).expect("dispatcher is alive");
    sender.send(2).expect("dispatcher is alive");
    assert_eq!(wakes.load(Ordering::SeqCst), 1);

    assert_eq!(dispatcher.try_recv(), Some(1));
    // One value is still queued, so the wake-up must be pending again.
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(dispatcher.try_recv(), Some(2));
    assert_eq!(wakes.load(Ordering::SeqCst), 2);

    sender.send(3).expect("dispatcher is alive");
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
    assert_eq!(dispatcher.drain(), vec![3]);
}

#[test]
fn wake_driven_draining_never_strands_values() {
    use std::sync::mpsc;
    use std::time::Duration;

    const PER_SENDER: usize = 10_000;

    let (wake_tx, wake_rx) = mpsc::channel::<()>();
    let (dispatcher, sender) = UiDispatcher::new(move || {
        let _ = wake_tx.send(());
    });

    let workers: Vec<_> = (0..2)
        .map(|_| {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for value in 0..PER_SENDER {
                    sender.send(value).expect("dispatcher is alive");
                }
            })
        })
        .collect();

    // Drain only when a wake-up arrives, like an event loop would. A lost
    // wake-up leaves values queued with no signal, and the wait times out.
    let mut received = 0usize;
    while received < PER_SENDER * 2 {
        wake_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("a wake-up arrives for every batch");
        received += dispatcher.drain().len();
    }
    assert!(dispatcher.is_empty());

    for worker in workers {
        worker.join().expect("no sender panics");
    }
}

#[test]
fn dispatcher_drains_values_produced_by_a_pool_task() {
    let pool = TaskPool::new(2).expect("workers start");
    let (dispatcher, sender) = UiDispatcher::new(|| {});

    let handle = pool.spawn(move || {
        for value in 0u32..4 {
            sender.send(value).expect("dispatcher is alive");
        }
    });
    handle.join().expect("no task panics");

    assert_eq!(dispatcher.drain(), vec![0, 1, 2, 3]);
}

#[test]
fn send_returns_the_value_after_the_dispatcher_is_dropped() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let (dispatcher, sender) = {
        let wakes = Arc::clone(&wakes);
        UiDispatcher::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
        })
    };
    drop(dispatcher);

    let error: UiSendError<String> = sender
        .send(String::from("late"))
        .expect_err("UI side is gone");

    assert_eq!(error.into_inner(), "late");
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
}

#[test]
fn drain_into_appends_to_an_existing_buffer() {
    let (dispatcher, sender) = UiDispatcher::new(|| {});
    sender.send(2).expect("dispatcher is alive");
    sender.send(3).expect("dispatcher is alive");

    let mut values = vec![1];
    dispatcher.drain_into(&mut values);

    assert_eq!(values, vec![1, 2, 3]);
}

#[test]
fn sender_obtained_from_the_dispatcher_keeps_the_channel_open() {
    let (dispatcher, sender) = UiDispatcher::new(|| {});
    drop(sender);

    dispatcher
        .sender()
        .send(9)
        .expect("the dispatcher keeps its own sender");

    assert_eq!(dispatcher.drain(), vec![9]);
}
