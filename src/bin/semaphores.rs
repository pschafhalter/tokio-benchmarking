use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use futures::{stream::FuturesUnordered, StreamExt};
use tokio::sync::{Mutex, Semaphore};

struct Producer {
    is_running: AtomicBool,
    lattice: Arc<Lattice>,
    frequency: f32,
}

impl Producer {
    fn new(lattice: Arc<Lattice>, frequency: f32) -> Self {
        Self {
            is_running: AtomicBool::new(false),
            lattice,
            frequency,
        }
    }

    async fn execute(&self) {
        self.is_running.store(true, Ordering::Relaxed);
        let sleep_duration = Duration::from_secs_f32(1.0 / self.frequency);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sleep_duration_2 = sleep_duration.clone();
        tokio::spawn(async move {
            loop {
                rx.recv().await.unwrap();
                // tokio::time::sleep(sleep_duration_2).await;
            }
        });

        while self.is_running.load(Ordering::Relaxed) {
            self.lattice.add_event(Event::new()).await;
            // tokio::task::yield_now().await;
            tx.send(()).unwrap();
            tokio::time::sleep(sleep_duration).await;
        }
    }

    fn shutdown(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }
}

struct Lattice {
    events: Arc<Mutex<VecDeque<Event>>>,
    semaphore: Semaphore,
}

impl Lattice {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
            semaphore: Semaphore::new(0),
        }
    }

    async fn add_event(&self, event: Event) {
        let mut events = self.events.lock().await;
        events.push_back(event);
        self.semaphore.add_permits(1);
    }

    async fn get_event(&self) -> Event {
        let permit = self.semaphore.acquire().await.unwrap();
        let mut events = self.events.lock().await;
        let event = events.pop_front().unwrap();
        permit.forget();
        event
    }
}

struct Event {
    create_time: Instant,
    lattice_time: Instant,
}

impl Event {
    fn new() -> Self {
        Self {
            create_time: Instant::now(),
            lattice_time: Instant::now(),
        }
    }
}

struct EventRunner {
    total_lattice_time: AtomicU64,
    num_events: AtomicUsize,
    is_running: AtomicBool,
    lattices: Vec<Arc<Lattice>>,
}

async fn get_event(lattice: Arc<Lattice>) -> (Event, Arc<Lattice>) {
    let event = lattice.get_event().await;
    (event, lattice)
}

impl EventRunner {
    fn new(lattices: Vec<Arc<Lattice>>) -> Self {
        Self {
            total_lattice_time: AtomicU64::new(0),
            num_events: AtomicUsize::new(0),
            is_running: AtomicBool::new(false),
            lattices,
        }
    }

    async fn execute(&self) {
        self.is_running.store(true, Ordering::Relaxed);

        let mut futures: FuturesUnordered<_> =
            self.lattices.iter().cloned().map(get_event).collect();

        while self.is_running.load(Ordering::Relaxed) {
            let (event, lattice) = futures.next().await.unwrap();
            futures.push(get_event(lattice));

            let lattice_time = event.lattice_time.elapsed().as_nanos() as u64;
            self.total_lattice_time
                .fetch_add(lattice_time, Ordering::Relaxed);
            self.num_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn shutdown(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }
}

#[tokio::main]
async fn main() {
    let num_producers = 16;
    let num_consumers = 19;
    let frequency = 30.0;

    let mut lattices = vec![];
    let mut producers = vec![];
    for _ in 0..num_producers {
        let lattice = Arc::new(Lattice::new());
        let producer = Arc::new(Producer::new(lattice.clone(), frequency));

        lattices.push(lattice);
        producers.push(producer.clone());

        tokio::spawn(async move {
            producer.execute().await;
        });
    }

    let mut consumers = vec![];
    for _ in 0..num_consumers {
        let consumer = Arc::new(EventRunner::new(lattices.clone()));
        consumers.push(consumer.clone());

        tokio::spawn(async move {
            consumer.execute().await;
        });
    }

    tokio::time::sleep(Duration::from_secs(10)).await;

    for producer in producers {
        producer.shutdown();
    }

    let mut total_lattice_time = 0;
    let mut num_events = 0;
    for consumer in consumers {
        consumer.shutdown();
        total_lattice_time += consumer.total_lattice_time.load(Ordering::Relaxed);
        num_events += consumer.num_events.load(Ordering::Relaxed);
    }

    let lattice_ns_per_event = total_lattice_time / num_events as u64;
    let lattice_us_per_event = lattice_ns_per_event as f32 / 1e3;

    println!("Spent {} us in lattices", lattice_us_per_event);
}
