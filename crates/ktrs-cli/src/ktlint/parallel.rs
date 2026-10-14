//! `Sequence<Callable<T>>.parallel(cb)` of `KtlintCommandLine`: items are processed on one worker per
//! core and their results handed to `report` in item order. Like upstream's lazy `takeWhile`, workers
//! stop taking items once `stop()` holds (checked before each item, so a few in-flight items finish).
//! Workers are named like the JVM's first `Executors` pool threads, which ktlint's log lines show.
//! `parallel_stream` is the same over a channel: the item stream (a file sequence still being walked)
//! is produced on an unnamed thread — `main` in the log, where the walk used to run.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

pub const WORKER_THREAD_PREFIX: &str = "pool-1-thread-";

pub fn parallel<I: Sync, T: Send>(
    items: &[I],
    stop: impl Fn() -> bool + Sync,
    work: impl Fn(&I) -> T + Sync,
    report: impl FnMut(&I, T) + Send,
) {
    if items.is_empty() {
        return;
    }
    let next = AtomicUsize::new(0);
    let done: Mutex<(usize, Vec<Option<T>>, _)> =
        Mutex::new((0, items.iter().map(|_| None).collect(), report));
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len());
    let worker_loop = || loop {
        if stop() {
            break;
        }
        let i = next.fetch_add(1, Ordering::SeqCst);
        let Some(item) = items.get(i) else { break };
        let result = work(item);
        let mut done = done.lock().unwrap();
        let (next_to_report, results, report) = &mut *done;
        results[i] = Some(result);
        while let Some(result) = results.get_mut(*next_to_report).and_then(Option::take) {
            report(&items[*next_to_report], result);
            *next_to_report += 1;
        }
    };
    if threads <= 1 {
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .name(format!("{WORKER_THREAD_PREFIX}1"))
                .spawn_scoped(s, worker_loop)
                .expect("failed to spawn a worker thread");
        });
        return;
    }
    let ready = AtomicBool::new(false);
    let main_thread = std::thread::current();
    std::thread::scope(|s| {
        let worker_1 = || {
            let i = next.fetch_add(1, Ordering::SeqCst);
            ready.store(true, Ordering::Release);
            main_thread.unpark();
            if !stop() && let Some(item) = items.get(i) {
                let result = work(item);
                let mut done = done.lock().unwrap();
                let (next_to_report, results, report) = &mut *done;
                results[i] = Some(result);
                while let Some(result) = results.get_mut(*next_to_report).and_then(Option::take) {
                    report(&items[*next_to_report], result);
                    *next_to_report += 1;
                }
            }
            worker_loop();
        };
        std::thread::Builder::new()
            .name(format!("{WORKER_THREAD_PREFIX}1"))
            .spawn_scoped(s, worker_1)
            .expect("failed to spawn a worker thread");
        while !ready.load(Ordering::Acquire) {
            std::thread::park();
        }
        for n in 2..=threads {
            std::thread::Builder::new()
                .name(format!("{WORKER_THREAD_PREFIX}{n}"))
                .spawn_scoped(s, worker_loop)
                .expect("failed to spawn a worker thread");
        }
    });
}

struct ReceiverQueue<I> {
    rx: Receiver<I>,
    next_index: usize,
    buffered: Option<I>,
    disconnected: bool,
}

enum NextJob<I> {
    Item(usize, I, bool /* more_work_possible */),
    Done,
}

impl<I> ReceiverQueue<I> {
    fn new(rx: Receiver<I>) -> Self {
        Self {
            rx,
            next_index: 0,
            buffered: None,
            disconnected: false,
        }
    }

    fn next_job(&mut self, stop: &dyn Fn() -> bool) -> NextJob<I> {
        if stop() {
            return NextJob::Done;
        }
        let item = if let Some(item) = self.buffered.take() {
            item
        } else if self.disconnected {
            return NextJob::Done;
        } else {
            match self.rx.recv() {
                Ok(item) => item,
                Err(_) => {
                    self.disconnected = true;
                    return NextJob::Done;
                }
            }
        };
        let index = self.next_index;
        self.next_index += 1;
        let more_work_possible = if self.disconnected {
            false
        } else {
            match self.rx.try_recv() {
                Ok(next_item) => {
                    self.buffered = Some(next_item);
                    true
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.disconnected = true;
                    false
                }
            }
        };
        NextJob::Item(index, item, more_work_possible)
    }
}

/// `parallel` over a channel: `produce` sends the items (already in report order) on a scope thread;
/// workers pull them under a shared lock — one recv per item, index = receive order — and report by
/// index like `parallel`. `stop()` ends both: workers stop taking and the producer (it gets the same
/// condition, plus a dead receiver) ends its stream early. Workers are spawned on-demand as items
/// are discovered to avoid spawning unused threads for small sequences.
pub fn parallel_stream<I: Send, T: Send>(
    produce: impl FnOnce(Sender<I>, &dyn Fn() -> bool) + Send,
    stop: impl Fn() -> bool + Sync,
    work: impl Fn(&I) -> T + Sync,
    report: impl FnMut(&I, T) + Send,
) {
    let (tx, rx) = channel::<I>();
    let rx = Mutex::new(ReceiverQueue::new(rx));
    let done: Mutex<(usize, BTreeMap<usize, (I, T)>, _)> =
        Mutex::new((0, BTreeMap::new(), report));
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());

    let spawned = AtomicUsize::new(1);
    let active_workers = AtomicUsize::new(1);
    const IDLE: u8 = 0;
    const NEED_WORKER: u8 = 1;
    const ALL_DONE: u8 = 2;
    let coordinator_state = AtomicU8::new(IDLE);
    let main_thread = std::thread::current();

    let worker_loop = || loop {
        let job = {
            let mut rx = rx.lock().unwrap();
            rx.next_job(&stop)
        };
        match job {
            NextJob::Item(i, item, more_work_possible) => {
                if more_work_possible && spawned.load(Ordering::Relaxed) < threads {
                    coordinator_state.store(NEED_WORKER, Ordering::Release);
                    main_thread.unpark();
                }
                let result = work(&item);
                let mut done = done.lock().unwrap();
                let (next_to_report, results, report) = &mut *done;
                results.insert(i, (item, result));
                while let Some((item, result)) = results.remove(next_to_report) {
                    report(&item, result);
                    *next_to_report += 1;
                }
            }
            NextJob::Done => {
                if active_workers.fetch_sub(1, Ordering::AcqRel) == 1 {
                    coordinator_state.store(ALL_DONE, Ordering::Release);
                    main_thread.unpark();
                }
                break;
            }
        }
    };

    if threads <= 1 {
        std::thread::scope(|s| {
            s.spawn(|| produce(tx, &stop));
            std::thread::Builder::new()
                .name(format!("{WORKER_THREAD_PREFIX}1"))
                .spawn_scoped(s, worker_loop)
                .expect("failed to spawn a worker thread");
        });
        return;
    }

    std::thread::scope(|s| {
        s.spawn(|| produce(tx, &stop));
        std::thread::Builder::new()
            .name(format!("{WORKER_THREAD_PREFIX}1"))
            .spawn_scoped(s, worker_loop)
            .expect("failed to spawn a worker thread");

        let mut num_spawned = 1;
        while num_spawned < threads {
            while coordinator_state.load(Ordering::Acquire) == IDLE {
                std::thread::park();
            }
            let state = coordinator_state.swap(IDLE, Ordering::AcqRel);
            if state == ALL_DONE {
                break;
            }
            if state == NEED_WORKER && num_spawned < threads {
                num_spawned += 1;
                spawned.store(num_spawned, Ordering::Release);
                active_workers.fetch_add(1, Ordering::Relaxed);
                let n = num_spawned;
                std::thread::Builder::new()
                    .name(format!("{WORKER_THREAD_PREFIX}{n}"))
                    .spawn_scoped(s, worker_loop)
                    .expect("failed to spawn a worker thread");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parallel_empty_items() {
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel(
            &[] as &[i32],
            || false,
            |&x| x * 2,
            move |&x, r| rep.lock().unwrap().push((x, r)),
        );
        assert!(reported.lock().unwrap().is_empty());
    }

    #[test]
    fn parallel_single_item_runs_on_worker_1() {
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        let items = vec![42];
        parallel(
            &items,
            || false,
            |&x| (x, std::thread::current().name().unwrap_or("").to_owned()),
            move |&_, (x, thread)| rep.lock().unwrap().push((x, thread)),
        );
        let results = reported.lock().unwrap().clone();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 42);
        assert_eq!(results[0].1, "pool-1-thread-1");
    }

    #[test]
    fn parallel_first_item_always_processed_by_worker_1() {
        for _ in 0..10 {
            let items: Vec<usize> = (0..50).collect();
            let thread_names = Arc::new(Mutex::new(Vec::new()));
            let tn = Arc::clone(&thread_names);
            parallel(
                &items,
                || false,
                |_| std::thread::current().name().unwrap_or("").to_owned(),
                move |&i, thread| tn.lock().unwrap().push((i, thread)),
            );
            let results = thread_names.lock().unwrap().clone();
            assert_eq!(results.len(), 50);
            assert_eq!(results[0].0, 0);
            assert_eq!(
                results[0].1, "pool-1-thread-1",
                "first item must be processed by pool-1-thread-1"
            );
        }
    }

    #[test]
    fn parallel_stops_when_requested() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let sf = Arc::clone(&stop_flag);
        let items: Vec<usize> = (0..100).collect();
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel(
            &items,
            move || sf.load(Ordering::SeqCst),
            |&x| {
                if x >= 5 {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                x
            },
            move |&_, r| rep.lock().unwrap().push(r),
        );
        let count = reported.lock().unwrap().len();
        assert!(count < 100, "expected early stop, got {count}");
    }

    #[test]
    fn parallel_stream_first_item_processed_by_worker_1() {
        for _ in 0..5 {
            let thread_names = Arc::new(Mutex::new(Vec::new()));
            let tn = Arc::clone(&thread_names);
            parallel_stream(
                |tx, _| {
                    for i in 0..20 {
                        let _ = tx.send(i);
                    }
                },
                || false,
                |_| std::thread::current().name().unwrap_or("").to_owned(),
                move |&i, thread| tn.lock().unwrap().push((i, thread)),
            );
            let results = thread_names.lock().unwrap().clone();
            assert_eq!(results.len(), 20);
            assert_eq!(results[0].0, 0);
            assert_eq!(results[0].1, "pool-1-thread-1");
        }
    }

    #[test]
    fn parallel_stream_empty() {
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel_stream(
            |_tx, _| {},
            || false,
            |&x: &i32| x * 2,
            move |&x, r| rep.lock().unwrap().push((x, r)),
        );
        assert!(reported.lock().unwrap().is_empty());
    }

    #[test]
    fn parallel_stream_many_items_in_order() {
        let count = 500;
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel_stream(
            move |tx, _| {
                for i in 0..count {
                    let _ = tx.send(i);
                }
            },
            || false,
            |&x| x * 10,
            move |&i, r| rep.lock().unwrap().push((i, r)),
        );
        let results = reported.lock().unwrap().clone();
        assert_eq!(results.len(), count);
        for (idx, &(i, r)) in results.iter().enumerate() {
            assert_eq!(i, idx);
            assert_eq!(r, idx * 10);
        }
    }

    #[test]
    fn parallel_stream_stops_when_requested() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let sf = Arc::clone(&stop_flag);
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel_stream(
            |tx, stop| {
                for i in 0..1000 {
                    if stop() {
                        break;
                    }
                    if tx.send(i).is_err() {
                        break;
                    }
                }
            },
            move || sf.load(Ordering::SeqCst),
            |&x| {
                if x >= 10 {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                x
            },
            move |&_, r| rep.lock().unwrap().push(r),
        );
        let count = reported.lock().unwrap().len();
        assert!(count < 1000, "expected early stop, got {count}");
    }

    #[test]
    fn parallel_stream_out_of_order_completion() {
        let count = 100;
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel_stream(
            move |tx, _| {
                for i in 0..count {
                    let _ = tx.send(i);
                }
            },
            || false,
            |&x| {
                // Skew execution time so higher indexed tasks finish before lower ones
                if x % 2 == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                x * 3
            },
            move |&i, r| rep.lock().unwrap().push((i, r)),
        );
        let results = reported.lock().unwrap().clone();
        assert_eq!(results.len(), count);
        for (idx, &(i, r)) in results.iter().enumerate() {
            assert_eq!(i, idx);
            assert_eq!(r, idx * 3);
        }
    }

    #[test]
    fn parallel_stream_single_item() {
        let reported = Arc::new(Mutex::new(Vec::new()));
        let rep = Arc::clone(&reported);
        parallel_stream(
            |tx, _| {
                let _ = tx.send(42);
            },
            || false,
            |&x| (x, std::thread::current().name().unwrap_or("").to_owned()),
            move |&_, (x, thread)| rep.lock().unwrap().push((x, thread)),
        );
        let results = reported.lock().unwrap().clone();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 42);
        assert_eq!(results[0].1, "pool-1-thread-1");
    }

    #[test]
    fn parallel_stream_two_items() {
        let thread_names = Arc::new(Mutex::new(Vec::new()));
        let tn = Arc::clone(&thread_names);
        parallel_stream(
            |tx, _| {
                let _ = tx.send(10);
                let _ = tx.send(20);
            },
            || false,
            |&x| (x, std::thread::current().name().unwrap_or("").to_owned()),
            move |&_, (x, thread)| tn.lock().unwrap().push((x, thread)),
        );
        let results = thread_names.lock().unwrap().clone();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 10);
        assert_eq!(results[0].1, "pool-1-thread-1");
        assert_eq!(results[1].0, 20);
    }
}

