use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::watch;

/// Callback invoked when a push makes the queue non-empty.
type OnNewFront<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// A replay-first event channel.
///
/// Events are stored in a FIFO queue. Subscribers are notified of the **front**
/// event only. Late subscribers immediately see the front event on subscribe
/// (replay). When the front event is marked done, it is removed and the next
/// event (if any) becomes the new front and is broadcast.
pub struct EventChannel<T: Clone + Send + Sync> {
    queue: Mutex<VecDeque<T>>,
    watch_tx: watch::Sender<Option<T>>,
    on_new_front: Mutex<Option<OnNewFront<T>>>,
}

impl<T: Clone + Send + Sync> EventChannel<T> {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            watch_tx: watch::Sender::new(None),
            on_new_front: Mutex::new(None),
        }
    }

    /// Registers a callback that runs when a push makes the queue non-empty.
    ///
    /// Fires inline on the pushing thread; keep it cheap and non-blocking.
    pub fn set_on_new_front(&self, callback: OnNewFront<T>) {
        *self
            .on_new_front
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(callback);
    }

    /// Adds an event to the back of the queue.
    /// If the queue was empty, broadcasts the new front (this event) to subscribers
    /// and runs the `on_new_front` callback.
    pub fn push(&self, event: T) {
        let front = {
            let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
            let was_empty = queue.is_empty();
            queue.push_back(event);
            if was_empty {
                queue.front().cloned()
            } else {
                None
            }
        };
        if let Some(front) = front {
            self.watch_tx.send_modify(|val| {
                *val = Some(front.clone());
            });
            // Cloned out so the callback never runs under the lock.
            if let Some(on_new_front) = self
                .on_new_front
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
            {
                on_new_front(&front);
            }
        }
    }

    /// Returns a clone of the front event, or `None` if the queue is empty.
    pub fn peek(&self) -> Option<T> {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .front()
            .cloned()
    }

    /// Removes the front event and broadcasts the next one (or `None`).
    pub fn mark_done(&self) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.pop_front();
        let next = queue.front().cloned();
        self.watch_tx.send_modify(|val| {
            *val = next;
        });
    }

    /// Subscribes to front-event notifications.
    /// Immediately sees the current front event (replay).
    pub fn subscribe(&self) -> watch::Receiver<Option<T>> {
        self.watch_tx.subscribe()
    }
}

impl<T: Clone + Send + Sync> Default for EventChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_callback_fires_when_queue_becomes_nonempty() {
        let channel = EventChannel::<u32>::new();
        let fired: Arc<Mutex<Vec<u32>>> = Arc::default();
        let collector = Arc::clone(&fired);
        channel.set_on_new_front(Arc::new(move |event: &u32| {
            collector.lock().unwrap().push(*event);
        }));

        // First push: queue transitions empty -> non-empty, callback fires.
        channel.push(1);
        // Push onto a non-empty queue and mark_done do not fire again.
        channel.push(2);
        channel.mark_done();
        assert_eq!(*fired.lock().unwrap(), vec![1]);

        // After the queue drains, the next push fires again.
        channel.mark_done();
        channel.push(3);
        assert_eq!(*fired.lock().unwrap(), vec![1, 3]);
    }

    #[test]
    fn test_queue_ordering_and_mark_done() {
        let channel = EventChannel::<u32>::new();

        assert_eq!(channel.peek(), None);

        channel.push(1);
        channel.push(2);
        channel.push(3);

        assert_eq!(channel.peek(), Some(1));

        channel.mark_done();
        assert_eq!(channel.peek(), Some(2));

        channel.mark_done();
        assert_eq!(channel.peek(), Some(3));

        channel.mark_done();
        assert_eq!(channel.peek(), None);
    }

    #[test]
    fn test_mark_done_on_empty_queue_no_panic() {
        let channel = EventChannel::<u32>::new();
        channel.mark_done();
        channel.mark_done();
        assert_eq!(channel.peek(), None);
    }

    #[test]
    fn test_replay_on_subscribe() {
        let channel = EventChannel::<u32>::new();

        channel.push(42);

        let rx = channel.subscribe();
        assert_eq!(*rx.borrow(), Some(42));

        channel.push(99);
        assert_eq!(*rx.borrow(), Some(42));

        channel.mark_done();
        assert_eq!(*rx.borrow(), Some(99));
    }

    #[tokio::test]
    async fn test_subscriber_notified_on_push() {
        let channel = EventChannel::<u32>::new();
        let mut rx = channel.subscribe();

        assert_eq!(*rx.borrow(), None);

        channel.push(7);

        assert!(rx.changed().await.is_ok());
        assert_eq!(*rx.borrow(), Some(7));
    }

    #[tokio::test]
    async fn test_mark_done_notifies_next_event() {
        let channel = EventChannel::<u32>::new();
        let mut rx = channel.subscribe();

        channel.push(1);
        channel.push(2);

        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), Some(1));

        channel.mark_done();
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), Some(2));

        channel.mark_done();
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), None);
    }

    #[tokio::test]
    async fn test_push_to_empty_after_mark_done_notifies() {
        let channel = EventChannel::<u32>::new();
        let mut rx = channel.subscribe();

        channel.push(1);
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), Some(1));

        channel.mark_done();
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), None);

        channel.push(2);
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), Some(2));
    }
}
