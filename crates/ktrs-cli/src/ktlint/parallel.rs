//! `Sequence<Callable<T>>.parallel(cb)` of `KtlintCommandLine`: items are processed on one worker per
//! core and their results handed to `report` in item order. Like upstream's lazy `takeWhile`, workers
//! stop taking items once `stop()` holds (checked before each item, so a few in-flight items finish).
//! Workers are named like the JVM's first `Executors` pool threads, which ktlint's log lines show.
//! `parallel_stream` is the same over a channel: the item stream (a file sequence still being walked)
//! is produced on an unnamed thread — `main` in the log, where the walk used to run.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

pub const WORKER_THREAD_PREFIX: &str = "pool-1-thread-";

pub fn parallel<I: Sync, T: Send>(
    items: &[I],
    stop: impl Fn() -> bool + Sync,
    work: impl Fn(&I) -> T + Sync,
    report: impl FnMut(&I, T) + Send,
) {
    let next = AtomicUsize::new(0);
    let done: Mutex<(usize, Vec<Option<T>>, _)> =
        Mutex::new((0, items.iter().map(|_| None).collect(), report));
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len().max(1));
    std::thread::scope(|s| {
        for n in 1..=threads {
            let worker = || {
                loop {
                    if stop() {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(item) = items.get(i) else { break };
                    let result = work(item);
                    let mut done = done.lock().unwrap();
                    let (next_to_report, results, report) = &mut *done;
                    results[i] = Some(result);
                    while let Some(result) = results.get_mut(*next_to_report).and_then(Option::take)
                    {
                        report(&items[*next_to_report], result);
                        *next_to_report += 1;
                    }
                }
            };
            std::thread::Builder::new()
                .name(format!("{WORKER_THREAD_PREFIX}{n}"))
                .spawn_scoped(s, worker)
                .expect("failed to spawn a worker thread");
        }
    });
}

/// `parallel` over a channel: `produce` sends the items (already in report order) on a scope thread;
/// workers pull them under a shared lock — one recv per item, index = receive order — and report by
/// index like `parallel`. `stop()` ends both: workers stop taking and the producer (it gets the same
/// condition, plus a dead receiver) ends its stream early.
pub fn parallel_stream<I: Send, T: Send>(
    produce: impl FnOnce(Sender<I>, &dyn Fn() -> bool) + Send,
    stop: impl Fn() -> bool + Sync,
    work: impl Fn(&I) -> T + Sync,
    report: impl FnMut(&I, T) + Send,
) {
    let (tx, rx) = channel::<I>();
    let rx: Mutex<(Receiver<I>, usize)> = Mutex::new((rx, 0));
    let done: Mutex<(usize, Vec<Option<(I, T)>>, _)> = Mutex::new((0, Vec::new(), report));
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    std::thread::scope(|s| {
        s.spawn(|| produce(tx, &stop));
        for n in 1..=threads {
            let worker = || loop {
                let (i, item) = {
                    let mut rx = rx.lock().unwrap();
                    if stop() {
                        break;
                    }
                    match rx.0.recv() {
                        Ok(item) => {
                            let i = rx.1;
                            rx.1 += 1;
                            (i, item)
                        }
                        Err(_) => break,
                    }
                };
                let result = work(&item);
                let mut done = done.lock().unwrap();
                let (next_to_report, results, report) = &mut *done;
                if results.len() <= i {
                    results.resize_with(i + 1, || None);
                }
                results[i] = Some((item, result));
                while let Some((item, result)) =
                    results.get_mut(*next_to_report).and_then(Option::take)
                {
                    report(&item, result);
                    *next_to_report += 1;
                }
            };
            std::thread::Builder::new()
                .name(format!("{WORKER_THREAD_PREFIX}{n}"))
                .spawn_scoped(s, worker)
                .expect("failed to spawn a worker thread");
        }
    });
}
