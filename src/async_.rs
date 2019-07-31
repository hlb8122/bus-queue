use super::*;
use futures::prelude::*;
use futures::{task::AtomicTask, Async, AsyncSink};
use futures::sync::mpsc;
use futures::task;

#[derive(Debug)]
pub struct Publisher<T: Send> {
    bare_publisher: BarePublisher<T>,
    waker: Waker<AtomicTask>,
}
#[derive(Debug)]
pub struct Subscriber<T: Send> {
    bare_subscriber: BareSubscriber<T>,
    sleeper: Sleeper<AtomicTask>,
}

pub fn channel<T: Send>(size: usize) -> (Publisher<T>, Subscriber<T>) {
    let (bare_publisher, bare_subscriber) = bare_channel(size);
    let (waker, sleeper) = alarm(AtomicTask::new());
    (
        Publisher {
            bare_publisher,
            task: task::current(),
            waker,
        },
        Subscriber {
            bare_subscriber,
            sleeper,
        },
    )
}
impl<T: Send> Publisher<T> {
    fn wake_all(&self) {
        for sleeper in self.waker.sleepers.iter() {
            sleeper.notify();
        }
    }
}

impl<T: Send> GetSubCount for Publisher<T> {
    fn get_sub_count(&self) -> usize {
        self.bare_publisher.get_sub_count()
    }
}

impl<T: Send> Sink for Publisher<T> {
    type SinkItem = T;
    type SinkError = SendError<T>;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.waker.register_receivers();
        self.bare_publisher.broadcast(item).map(|_| {
            self.wake_all();
            AsyncSink::Ready
        })
    }
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.poll_complete()
    }
}

impl<T: Send> Drop for Publisher<T> {
    fn drop(&mut self) {
        self.close().unwrap();
    }
}

impl<T: Send> PartialEq for Publisher<T> {
    fn eq(&self, other: &Publisher<T>) -> bool {
        self.bare_publisher == other.bare_publisher
    }
}

impl<T: Send> Eq for Publisher<T> {}

impl<T: Send> Stream for Subscriber<T> {
    type Item = Arc<T>;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.bare_subscriber.try_recv() {
            Ok(arc_object) => Ok(Async::Ready(Some(arc_object))),
            Err(error) => match error {
                TryRecvError::Empty => {
                    self.sleeper.sleeper.register();
                    Ok(Async::NotReady)
                }
                TryRecvError::Disconnected => Ok(Async::Ready(None)),
            },
        }
    }
}

impl<T: Send> Clone for Subscriber<T> {
    fn clone(&self) -> Self {
        let arc_t = Arc::new(AtomicTask::new());
        self.sleeper.sender.send(arc_t.clone());
        Self {
            bare_subscriber: self.bare_subscriber.clone(),
            sleeper: Sleeper {
                sender: self.sleeper.sender.clone(),
                sleeper: arc_t.clone(),
            },
        }
    }
}

impl<T: Send> PartialEq for Subscriber<T> {
    fn eq(&self, other: &Subscriber<T>) -> bool {
        self.bare_subscriber == other.bare_subscriber
    }
}

impl<T: Send> Eq for Subscriber<T> {}

/// Helper struct used by sync and async implementations to wake Tasks / Threads
#[derive(Debug)]
pub struct Waker<T> {
    /// Vector of Tasks / Threads to be woken up.
    pub sleepers: Vec<Arc<T>>,
    /// A mpsc Receiver used to receive Tasks / Threads to be registered.
    receiver: mpsc::Receiver<Arc<T>>,
}

/// Helper struct used by sync and async implementations to register Tasks / Threads to
/// be woken up.
#[derive(Debug)]
pub struct Sleeper<T> {
    /// Current Task / Thread to be woken up.
    pub sleeper: Arc<T>,
    /// mpsc Sender used to register Task / Thread.
    pub sender: mpsc::Sender<Arc<T>>,
}

impl<T> Waker<T> {
    /// Register all the Tasks / Threads sent for registration.
    pub fn register_receivers(&mut self) -> impl Future<Item=()> {
        for receiver in self.receiver.recv() {
            self.sleepers.push(receiver);
        }
    }
}

/// Function used to create a ( Waker, Sleeper ) tuple.
pub fn alarm<T>(current: T) -> (Waker<T>, Sleeper<T>) {
    let mut vec = Vec::new();
    let (sender, receiver) = mpsc::channel(10);
    let arc_t = Arc::new(current);
    vec.push(arc_t.clone());
    (
        Waker {
            sleepers: vec,
            receiver,
        },
        Sleeper {
            sleeper: arc_t,
            sender,
        },
    )
}
