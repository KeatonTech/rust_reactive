use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub(crate) struct StreamImpl<T> {
    highest_id: u16,
    is_alive: bool,
    on_emit: BTreeMap<u16, Box<dyn Fn(Arc<T>) + Send + Sync>>,
    pub(crate) extra_fields: Option<Box<dyn Any + Send + Sync + 'static>>,
}

/// Streams are objects that emit events in sequence as they are created. Streams are
/// similar to Iterators in Rust in that both represent a sequence of values and both
/// can be modified by 'pipe' functions like `map` and `filter`. The difference is that
/// all values of an iterator are known immediately (or, at least, execution will block
/// while the next item is retrieved), whereas it would not be uncommon for a stream to
/// live for the entire duration of a program, emitting new values from time-to-time.
///
/// # Examples
///
/// ```
/// let stream_host: epoxy_streams::Sink<i32> = epoxy_streams::Sink::new();
/// let stream = stream_host.get_stream();
/// {
///     let _sub = stream.subscribe(|val| {val;});
///     assert_eq!(stream.count_subscribers(), 1);
/// }
/// assert_eq!(stream.count_subscribers(), 0);
/// ```
///
/// ```
/// use std::sync::{Arc, Mutex};
///
/// let stream_host: epoxy_streams::Sink<i32> = epoxy_streams::Sink::new();
/// let stream = stream_host.get_stream();
///
/// let last_value = Arc::new(Mutex::new(0_i32));
/// let last_value_write = last_value.clone();
///
/// let subscription = stream.subscribe(move |val| {
///     *last_value_write.lock().unwrap() = *val;
/// });
///
/// stream_host.emit(1);
/// assert_eq!(*last_value.lock().unwrap(), 1);
///
/// stream_host.emit(100);
/// assert_eq!(*last_value.lock().unwrap(), 100);
/// ```
pub struct Stream<T> {
    pub(crate) pointer: Arc<Mutex<StreamImpl<T>>>,
}

/// A Subscription object ties a stream to a listener function such that the listener function is
/// run whenever a new value is added to the stream. When the Subscription object is destroyed
/// the listener function will stop getting called.
///
/// # Examples
///
/// ```
/// let stream_host: epoxy_streams::Sink<i32> = epoxy_streams::Sink::new();
/// let stream = stream_host.get_stream();
/// {
///     let _subscription = stream.subscribe(|val| {val;});
///     assert_eq!(stream.count_subscribers(), 1);
/// }
/// assert_eq!(stream.count_subscribers(), 0);
/// ```
pub struct Subscription<T> {
    id: u16,
    pub(crate) stream: Stream<T>,
}

/// A Sink is an object used to create a Stream. If you have ever visited a kitchen or bathroom
/// you have probably observed this phenomena already. In more technical terms, Sinks are the
/// 'write' part of functional reactive programming, and Streams are the 'read' part.
///
/// # Examples
/// ```
/// use std::sync::{Arc, Mutex};
///
/// let stream_host: epoxy_streams::Sink<i32> = epoxy_streams::Sink::new();
/// let stream = stream_host.get_stream();
///
/// let last_value = Arc::new(Mutex::new(0_i32));
/// let last_value_write = last_value.clone();
///
/// let subscription = stream.subscribe(move |val| {
///     *last_value_write.lock().unwrap() = *val;
/// });
///
/// stream_host.emit(1);
/// assert_eq!(*last_value.lock().unwrap(), 1);
///
/// stream_host.emit(100);
/// assert_eq!(*last_value.lock().unwrap(), 100);
/// ```
pub struct Sink<T> {
    stream: Stream<T>,
}

impl<T> Clone for Stream<T> {
    fn clone(&self) -> Self {
        Stream {
            pointer: Arc::clone(&self.pointer),
        }
    }
}

impl<T> StreamImpl<T> {
    fn subscribe<F>(&mut self, listener: F) -> u16
    where
        F: Fn(Arc<T>),
        F: Send,
        F: Sync,
        F: 'static,
    {
        let new_subscription_id = self.highest_id;
        self.highest_id += 1;
        self.on_emit.insert(new_subscription_id, Box::new(listener));
        new_subscription_id
    }

    pub(crate) fn emit_rc(&self, value: Arc<T>) {
        for (_id, call) in &self.on_emit {
            call(value.clone())
        }
    }
}

impl<T> Stream<T> {
    /// Subscribing to a stream will cause the given 'listener' function to be executed whenever
    /// a new object is added to the stream. This listener function has a static lifetime because
    /// it lives as long as the returned Subscription object, which means that in most cases if the
    /// given function needs to capture any scope from its environment it will need to be used with
    /// Rust's `move` annotation.
    pub fn subscribe<F>(&self, listener: F) -> Subscription<T>
    where
        F: Fn(Arc<T>),
        F: Send,
        F: Sync,
        F: 'static,
    {
        let mut stream_mut = match self.pointer.lock() {
            Ok(mut_ref) => mut_ref,
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        };

        Subscription {
            id: stream_mut.subscribe(listener),
            stream: self.clone(),
        }
    }

    /// Usually subscriptions are removed by simply letting the Subscription object fall out of
    /// scope, but this declarative API is provided as well as it may be more readable in some
    /// situations.
    pub fn unsubscribe(&self, _subscription: Subscription<T>) {
        // By moving the subscription into this function it will automatically get dropped,
        // thereby calling the internal unsubscribe_by_id function.
    }

    /// Dispatches each item in a stream into a Sink. This will last as long as
    /// the returned Subscription object stays in scope.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    ///
    /// let stream_host_1: epoxy_streams::Sink<i32> = epoxy_streams::Sink::new();
    /// let stream = stream_host_1.get_stream();
    ///
    /// let stream_host_2: epoxy_streams::Sink<i32> = epoxy_streams::Sink::new();
    /// let subscription = stream_host_2.get_stream().pipe_into(&stream_host_1);
    ///
    /// let last_value = Arc::new(Mutex::new(0_i32));
    /// let last_value_write = last_value.clone();
    /// let subscription = stream.subscribe(move |val| {
    ///     *last_value_write.lock().unwrap() = *val;
    /// });
    ///
    /// stream_host_2.emit(1);
    /// assert_eq!(*last_value.lock().unwrap(), 1);
    ///
    /// stream_host_2.emit(100);
    /// assert_eq!(*last_value.lock().unwrap(), 100);
    /// ```
    pub fn pipe_into(&self, sink: &Sink<T>) -> Subscription<T>
    where
        T: 'static,
    {
        let stream = sink.stream.clone();
        self.subscribe(move |item| stream.emit_rc(item))
    }

    /// Returns the total number of subscribers listening to this stream, includes any derived
    /// streams (ones created with a pipe operation like `map` or `filter`).
    pub fn count_subscribers(&self) -> usize {
        let stream = match self.pointer.lock() {
            Ok(stream_impl) => stream_impl,
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        };
        stream.on_emit.len()
    }

    fn unsubscribe_by_id(&self, subscription_id: u16) {
        let mut stream_mut = match self.pointer.lock() {
            Ok(mut_ref) => mut_ref,
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        };
        stream_mut.on_emit.remove(&subscription_id);
    }

    // PRIVATE FUNCTIONS

    pub(crate) fn new() -> Stream<T> {
        Stream {
            pointer: Arc::new(Mutex::new(StreamImpl {
                highest_id: 0_u16,
                is_alive: true,
                on_emit: BTreeMap::new(),
                extra_fields: None,
            })),
        }
    }

    pub(crate) fn new_with_fields<FieldsType>(fields: FieldsType) -> Stream<T>
    where
        FieldsType: 'static,
        FieldsType: Send,
        FieldsType: Sync,
    {
        Stream {
            pointer: Arc::new(Mutex::new(StreamImpl {
                highest_id: 0_u16,
                is_alive: true,
                on_emit: BTreeMap::new(),
                extra_fields: Some(Box::new(fields)),
            })),
        }
    }

    pub(crate) fn emit_rc(&self, value: Arc<T>) {
        match self.pointer.lock() {
            Ok(stream_impl) => stream_impl.emit_rc(value),
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        }
    }

    pub(crate) fn read_extra_fields<ExtraFieldsType, RetType, FnType>(&self, cb: FnType) -> RetType
    where
        ExtraFieldsType: 'static,
        ExtraFieldsType: Send,
        ExtraFieldsType: Sync,
        RetType: 'static,
        FnType: FnOnce(&ExtraFieldsType) -> RetType,
    {
        match self.pointer.lock() {
            Ok(stream_impl) => {
                if let Some(extra_field_box) = &stream_impl.extra_fields {
                    if let Some(fields) = extra_field_box.downcast_ref::<ExtraFieldsType>() {
                        return cb(&*fields);
                    }
                }
                panic!("Invalid type for derived stream field.");
            },
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        }
    }

    pub(crate) fn mutate_extra_fields<ExtraFieldsType, FnType>(&self, cb: FnType)
    where
        ExtraFieldsType: 'static,
        ExtraFieldsType: Send,
        ExtraFieldsType: Sync,
        FnType: FnOnce(&mut ExtraFieldsType),
    {
        match self.pointer.lock() {
            Ok(mut stream_impl) => {
                if let Some(extra_field_box) = &mut stream_impl.extra_fields {
                    if let Some(fields) = extra_field_box.downcast_mut::<ExtraFieldsType>() {
                        return cb(&mut *fields);
                    }
                }
                panic!("Invalid type for derived stream field.");
            },
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        }
    }
}

impl<T> Sink<T> {
    pub fn new() -> Sink<T> {
        Sink {
            stream: Stream::new(),
        }
    }

    /// Returns the Stream that emits values from this Sink. Usually the Stream will be exposed as
    /// a public API while the Sink will be kept private, however there are certainly exceptions
    /// to this pattern.
    pub fn get_stream(&self) -> Stream<T> {
        self.stream.clone()
    }

    /// Emits a new value from this Sink, which will broadcast out to any Subscriber to the stream
    /// returned by the `get_stream` function.
    pub fn emit(&self, value: T) {
        self.emit_rc(Arc::new(value))
    }

    /// Same logic as `emit`, but takes an existing Arc pointer (Epoxy streams use Arc pointers
    /// internally, so this saves a Copy).
    pub fn emit_rc(&self, value: Arc<T>) {
        self.stream.emit_rc(value)
    }
}

impl<T> Drop for Sink<T> {
    fn drop(&mut self) {
        let mut stream_mut = match self.stream.pointer.lock() {
            Ok(mut_ref) => mut_ref,
            Err(err) => panic!("Stream mutex poisoned: {}", err),
        };
        stream_mut.is_alive = false;
    }
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        self.stream.unsubscribe_by_id(self.id)
    }
}
