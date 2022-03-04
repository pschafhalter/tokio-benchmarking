use std::{
    fmt::Debug,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::future::select_all;
use tokio::{
    runtime::Builder,
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Mutex,
    },
    task,
};

struct Event {
    create_time: Instant,
    payload: Vec<u8>,
    callback: Box<dyn Fn() -> () + Send + Sync>,
    hop_num: usize,
}

impl Event {
    pub fn new(hop_num: usize, payload_size: usize) -> Self {
        let payload = vec![0u8; payload_size];
        Self {
            create_time: Instant::now(),
            payload,
            hop_num,
            callback: Box::new(|| {
                let now = Instant::now();
                while now.elapsed() < Duration::from_micros(100) {}
            }),
        }
    }
}

impl Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Event")
            .field("create_time", &self.create_time)
            .field("payload", &self.payload)
            .field("hop_num", &self.hop_num)
            .finish()
    }
}

struct RunQueue {
    tx: UnboundedSender<Event>,
    rx: Arc<Mutex<UnboundedReceiver<Event>>>,
}

impl RunQueue {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }

    fn send(&self, event: Event) {
        self.tx.send(event).unwrap();
    }

    async fn recv(&self) -> Event {
        self.rx.lock().await.recv().await.unwrap()
    }
}

async fn producer(run_queue: Arc<RunQueue>, frequency: f32) {
    let sleep_dur = Duration::from_secs_f32(1.0 / frequency);

    loop {
        run_queue.send(Event::new(0, 1_000_000));
        tokio::time::sleep(sleep_dur).await;
    }
}

async fn consumer(run_queue: Arc<RunQueue>, stats_tx: UnboundedSender<Duration>) {
    loop {
        let event = run_queue.recv().await;

        // tokio::task::block_in_place(event.callback);
        // tokio::task::block_in_place(|| {});

        stats_tx.send(event.create_time.elapsed()).unwrap();
        (event.callback)();
    }
}

async fn timing_task(iters: usize, mut timing_rx: UnboundedReceiver<Duration>) {
    // warmup
    for _ in 0..100 {
        timing_rx.recv().await.unwrap();
    }

    let mut total_dur = Duration::from_secs(0);
    for _ in 0..iters {
        let dur = timing_rx.recv().await.unwrap();
        total_dur += dur;
    }

    let avg_ns = total_dur.as_nanos() / iters as u128;
    let avg_ms = avg_ns as f64 / 1e6;
    println!("avg ms: {}", avg_ms);
}

fn simple_consumer(event: Event, stats_tx: UnboundedSender<Duration>) {
    stats_tx.send(event.create_time.elapsed()).unwrap();
    (event.callback)();
}

async fn consumer_manager(
    max_consumers: usize,
    run_queue: Arc<RunQueue>,
    stats_tx: UnboundedSender<Duration>,
) {
    let mut active_consumers = vec![];

    let mut rx = run_queue.rx.lock().await;
    loop {
        // let event = run_queue.recv().await;
        let event = rx.recv().await.unwrap();

        // while active_consumers.len() >= max_consumers {
        //     let (_, _, remaining_consumers) = select_all(active_consumers.drain(..)).await;
        //     active_consumers = remaining_consumers;
        // }

        let stats_tx_clone = stats_tx.clone();
        // active_consumers.push(tokio::task::spawn_blocking(move || {
        //     simple_consumer(event, stats_tx_clone);
        // }));
        // active_consumers.push(tokio::task::spawn(async {
        //     tokio::task::block_in_place(|| simple_consumer(event, stats_tx_clone));
        // }))
        active_consumers.push(tokio::task::spawn(async {
            simple_consumer(event, stats_tx_clone);
        }))
    }
}

async fn tokio_main(num_producers: usize, num_consumers: usize) {
    let run_queue = Arc::new(RunQueue::new());

    let (tx, rx) = mpsc::unbounded_channel::<Duration>();

    let timing_task = task::spawn(timing_task(1000, rx));

    for _ in 0..num_consumers {
        tokio::task::spawn(consumer(run_queue.clone(), tx.clone()));
    }
    // tokio::task::spawn(consumer_manager(
    //     num_consumers,
    //     run_queue.clone(),
    //     tx.clone(),
    // ));

    for _ in 0..num_producers {
        tokio::task::spawn(producer(run_queue.clone(), 30.0));
    }

    timing_task.await.unwrap();
}

fn main() {
    let runtime = Builder::new_multi_thread()
        .worker_threads(8)
        // .thread_name(format!("node-{}", self.id))
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(tokio_main(4, 2));
}
