//! Lock-free SPSC ring buffer for hot path decoupling.
//!
//! This ring buffer is designed for single-producer single-consumer
//! use between the trading thread (producer) and the background writer
//! thread (consumer).
//!
//! # Design
//!
//! - Fixed capacity, pre-allocated (power of 2 for fast modulo)
//! - Lock-free using atomic indices with Acquire/Release ordering
//! - Cache-line padding between head and tail to prevent false sharing
//! - Zero allocations in push/pop hot paths
//!
//! # Thread Safety
//!
//! - `try_push()` is only called from the producer thread
//! - `pop()` is only called from the consumer thread
//! - Concurrent push/pop is safe due to atomic ordering
//!
//! # Example
//!
//! ```
//! use blackbox::journal::RingBuffer;
//!
//! let rb = RingBuffer::<u64>::new(16);
//!
//! // Producer thread
//! assert!(rb.try_push(42));
//! assert!(rb.try_push(43));
//!
//! // Consumer thread
//! assert_eq!(rb.pop(), Some(42));
//! assert_eq!(rb.pop(), Some(43));
//! assert_eq!(rb.pop(), None);
//! ```

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cache line size for padding (64 bytes on most modern CPUs).
const CACHE_LINE_SIZE: usize = 64;

/// A record entry in the ring buffer for journal writes.
#[derive(Clone)]
pub struct BufferEntry {
    /// Record type.
    pub record_type: u16,
    /// Exchange ID.
    pub exchange_id: u8,
    /// Timestamp in microseconds.
    pub timestamp: i64,
    /// Payload data.
    pub payload: Vec<u8>,
}

impl BufferEntry {
    /// Create a new buffer entry.
    pub fn new(record_type: u16, exchange_id: u8, timestamp: i64, payload: Vec<u8>) -> Self {
        Self {
            record_type,
            exchange_id,
            timestamp,
            payload,
        }
    }
}

/// Padded atomic usize to prevent false sharing.
///
/// By padding to cache line size, we ensure that the head and tail
/// indices don't share a cache line, which would cause performance
/// degradation due to false sharing.
#[repr(align(64))]
struct PaddedAtomicUsize {
    value: AtomicUsize,
    /// Padding to fill cache line.
    _padding: [u8; CACHE_LINE_SIZE - std::mem::size_of::<AtomicUsize>()],
}

impl PaddedAtomicUsize {
    fn new(val: usize) -> Self {
        Self {
            value: AtomicUsize::new(val),
            _padding: [0u8; CACHE_LINE_SIZE - std::mem::size_of::<AtomicUsize>()],
        }
    }

    #[inline]
    fn load(&self, order: Ordering) -> usize {
        self.value.load(order)
    }

    #[inline]
    fn store(&self, val: usize, order: Ordering) {
        self.value.store(val, order)
    }
}

/// Lock-free single-producer single-consumer ring buffer.
///
/// This is the core data structure for decoupling the hot trading path
/// from the background disk I/O. Records are pushed from the trading
/// thread and popped by the background writer thread.
///
/// # Performance
///
/// - `try_push()`: O(1), lock-free, zero allocations
/// - `pop()`: O(1), lock-free, zero allocations
/// - Cache-line padding prevents false sharing between producer and consumer
///
/// # Memory Ordering
///
/// Uses Acquire/Release semantics:
/// - Producer: Release when incrementing head (makes data visible)
/// - Consumer: Acquire when reading head (sees producer's writes)
/// - Consumer: Release when incrementing tail
/// - Producer: Acquire when reading tail
pub struct RingBuffer<T> {
    /// Storage for entries (pre-allocated).
    buffer: UnsafeCell<Vec<MaybeUninit<T>>>,
    /// Capacity (power of 2 for fast modulo).
    capacity: usize,
    /// Mask for fast index calculation (capacity - 1).
    mask: usize,
    /// Write index (producer only writes, consumer reads).
    head: PaddedAtomicUsize,
    /// Read index (consumer only writes, producer reads).
    tail: PaddedAtomicUsize,
}

impl<T> RingBuffer<T> {
    /// Create a new ring buffer with the given capacity.
    ///
    /// Capacity will be rounded up to the nearest power of 2.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Desired capacity (will be rounded up to power of 2)
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::journal::RingBuffer;
    ///
    /// let rb = RingBuffer::<i32>::new(10);
    /// assert_eq!(rb.capacity(), 16); // Rounded to power of 2
    /// ```
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(2).next_power_of_two();
        let mask = capacity - 1;

        // Pre-allocate the buffer with uninitialized memory
        let mut buffer = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            buffer.push(MaybeUninit::uninit());
        }

        Self {
            buffer: UnsafeCell::new(buffer),
            capacity,
            mask,
            head: PaddedAtomicUsize::new(0),
            tail: PaddedAtomicUsize::new(0),
        }
    }

    /// Try to push an item, returning false if full.
    ///
    /// This is the hot path operation - must be lock-free and zero-allocation.
    ///
    /// # Arguments
    ///
    /// * `item` - The item to push
    ///
    /// # Returns
    ///
    /// `true` if the item was successfully pushed, `false` if the buffer is full.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::journal::RingBuffer;
    ///
    /// let rb = RingBuffer::<i32>::new(4);
    /// assert!(rb.try_push(1));
    /// assert!(rb.try_push(2));
    /// assert!(rb.try_push(3));
    /// // Buffer is now full (capacity 4, but one slot reserved)
    /// assert!(!rb.try_push(4));
    /// ```
    #[inline]
    pub fn try_push(&self, item: T) -> bool {
        // Load current head (we're the only writer to head)
        let head = self.head.load(Ordering::Relaxed);

        // Load tail with Acquire to see consumer's updates
        let tail = self.tail.load(Ordering::Acquire);

        // Calculate next head position
        let next_head = (head + 1) & self.mask;

        // Check if full (next_head would equal tail)
        if next_head == tail {
            return false;
        }

        // SAFETY: We're the only writer to buffer[head], and we've verified
        // there's space. The slot at head is not currently being read.
        unsafe {
            let buffer = &mut *self.buffer.get();
            buffer[head].write(item);
        }

        // Release: make the write visible before updating head
        self.head.store(next_head, Ordering::Release);

        true
    }

    /// Try to pop an item, returning None if empty.
    ///
    /// This is called by the consumer thread to drain items.
    ///
    /// # Returns
    ///
    /// `Some(item)` if an item was available, `None` if the buffer is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use blackbox::journal::RingBuffer;
    ///
    /// let rb = RingBuffer::<i32>::new(4);
    /// rb.try_push(42);
    /// assert_eq!(rb.pop(), Some(42));
    /// assert_eq!(rb.pop(), None);
    /// ```
    #[inline]
    pub fn pop(&self) -> Option<T> {
        // Load current tail (we're the only writer to tail)
        let tail = self.tail.load(Ordering::Relaxed);

        // Load head with Acquire to see producer's updates
        let head = self.head.load(Ordering::Acquire);

        // Check if empty
        if head == tail {
            return None;
        }

        // SAFETY: We're the only reader from buffer[tail], and we've verified
        // there's data available. The slot at tail is not currently being written.
        let item = unsafe {
            let buffer = &*self.buffer.get();
            buffer[tail].assume_init_read()
        };

        // Calculate next tail position
        let next_tail = (tail + 1) & self.mask;

        // Release: make the read complete before updating tail
        self.tail.store(next_tail, Ordering::Release);

        Some(item)
    }

    /// Check if the buffer is empty.
    ///
    /// Note: This is a snapshot - the state may change immediately after.
    #[inline]
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head == tail
    }

    /// Check if the buffer is full.
    ///
    /// Note: This is a snapshot - the state may change immediately after.
    #[inline]
    pub fn is_full(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        ((head + 1) & self.mask) == tail
    }

    /// Get the current number of items in the buffer.
    ///
    /// Note: This is a snapshot - the state may change immediately after.
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        (head.wrapping_sub(tail)) & self.mask
    }

    /// Get the capacity of the buffer.
    ///
    /// Note: The usable capacity is `capacity - 1` due to the full/empty
    /// disambiguation slot.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the maximum number of items that can be stored.
    ///
    /// This is `capacity - 1` due to the full/empty disambiguation slot.
    pub fn max_size(&self) -> usize {
        self.capacity - 1
    }

    /// Clear all items from the buffer.
    ///
    /// # Safety
    ///
    /// This should only be called when no other thread is accessing the buffer.
    /// In SPSC usage, call this only when both producer and consumer are stopped.
    pub fn clear(&self) {
        // Drop all items currently in the buffer
        while self.pop().is_some() {}
    }
}

// SAFETY: RingBuffer is safe to send between threads.
// The internal UnsafeCell is accessed safely through atomic synchronization.
unsafe impl<T: Send> Send for RingBuffer<T> {}

// SAFETY: RingBuffer can be shared between threads (via Arc) when used as SPSC.
// - Only one thread calls try_push (producer)
// - Only one thread calls pop (consumer)
// - Atomic operations provide synchronization
unsafe impl<T: Send> Sync for RingBuffer<T> {}

impl<T> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        // Drop any remaining items in the buffer
        while self.pop().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // ==================== Construction Tests ====================

    #[test]
    fn test_new_rounds_to_power_of_two() {
        let rb: RingBuffer<i32> = RingBuffer::new(10);
        assert_eq!(rb.capacity(), 16);

        let rb: RingBuffer<i32> = RingBuffer::new(1);
        assert_eq!(rb.capacity(), 2);

        let rb: RingBuffer<i32> = RingBuffer::new(16);
        assert_eq!(rb.capacity(), 16);

        let rb: RingBuffer<i32> = RingBuffer::new(17);
        assert_eq!(rb.capacity(), 32);
    }

    #[test]
    fn test_new_buffer_is_empty() {
        let rb: RingBuffer<i32> = RingBuffer::new(16);
        assert!(rb.is_empty());
        assert!(!rb.is_full());
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn test_max_size() {
        let rb: RingBuffer<i32> = RingBuffer::new(16);
        assert_eq!(rb.max_size(), 15); // capacity - 1
    }

    // ==================== Basic Push/Pop Tests ====================

    #[test]
    fn test_single_push_pop() {
        let rb = RingBuffer::new(16);
        assert!(rb.try_push(42));
        assert_eq!(rb.pop(), Some(42));
        assert_eq!(rb.pop(), None);
    }

    #[test]
    fn test_multiple_push_pop() {
        let rb = RingBuffer::new(16);

        for i in 0..10 {
            assert!(rb.try_push(i));
        }
        assert_eq!(rb.len(), 10);

        for i in 0..10 {
            assert_eq!(rb.pop(), Some(i));
        }
        assert!(rb.is_empty());
    }

    #[test]
    fn test_fifo_ordering() {
        let rb = RingBuffer::new(16);

        rb.try_push(1);
        rb.try_push(2);
        rb.try_push(3);

        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));
        assert_eq!(rb.pop(), Some(3));
    }

    #[test]
    fn test_interleaved_push_pop() {
        let rb = RingBuffer::new(16);

        rb.try_push(1);
        assert_eq!(rb.pop(), Some(1));

        rb.try_push(2);
        rb.try_push(3);
        assert_eq!(rb.pop(), Some(2));

        rb.try_push(4);
        assert_eq!(rb.pop(), Some(3));
        assert_eq!(rb.pop(), Some(4));
        assert_eq!(rb.pop(), None);
    }

    // ==================== Capacity and Full Tests ====================

    #[test]
    fn test_buffer_full() {
        let rb = RingBuffer::new(4); // capacity 4, max items 3

        assert!(rb.try_push(1));
        assert!(rb.try_push(2));
        assert!(rb.try_push(3));
        assert!(rb.is_full());
        assert!(!rb.try_push(4)); // Should fail - full
    }

    #[test]
    fn test_buffer_full_then_pop() {
        let rb = RingBuffer::new(4);

        // Fill the buffer
        assert!(rb.try_push(1));
        assert!(rb.try_push(2));
        assert!(rb.try_push(3));
        assert!(rb.is_full());

        // Pop one
        assert_eq!(rb.pop(), Some(1));
        assert!(!rb.is_full());

        // Should be able to push again
        assert!(rb.try_push(4));
        assert!(rb.is_full());

        // Verify order
        assert_eq!(rb.pop(), Some(2));
        assert_eq!(rb.pop(), Some(3));
        assert_eq!(rb.pop(), Some(4));
    }

    #[test]
    fn test_len_updates() {
        let rb = RingBuffer::new(16);

        assert_eq!(rb.len(), 0);
        rb.try_push(1);
        assert_eq!(rb.len(), 1);
        rb.try_push(2);
        assert_eq!(rb.len(), 2);
        rb.pop();
        assert_eq!(rb.len(), 1);
        rb.pop();
        assert_eq!(rb.len(), 0);
    }

    // ==================== Wrap-Around Tests ====================

    #[test]
    fn test_wrap_around() {
        let rb = RingBuffer::new(4); // capacity 4

        // Push 3 items (fill buffer)
        rb.try_push(1);
        rb.try_push(2);
        rb.try_push(3);

        // Pop 2
        assert_eq!(rb.pop(), Some(1));
        assert_eq!(rb.pop(), Some(2));

        // Push 2 more (will wrap around)
        rb.try_push(4);
        rb.try_push(5);

        // Verify all items
        assert_eq!(rb.pop(), Some(3));
        assert_eq!(rb.pop(), Some(4));
        assert_eq!(rb.pop(), Some(5));
        assert_eq!(rb.pop(), None);
    }

    #[test]
    fn test_extended_wrap_around() {
        let rb = RingBuffer::new(8);

        // Do many wrap-arounds
        for cycle in 0..10 {
            for i in 0..7 {
                assert!(
                    rb.try_push(cycle * 10 + i),
                    "Push failed at cycle {} item {}",
                    cycle,
                    i
                );
            }
            for i in 0..7 {
                let expected = cycle * 10 + i;
                assert_eq!(
                    rb.pop(),
                    Some(expected),
                    "Pop mismatch at cycle {} item {}",
                    cycle,
                    i
                );
            }
        }
    }

    // ==================== Clear Tests ====================

    #[test]
    fn test_clear() {
        let rb = RingBuffer::new(16);

        rb.try_push(1);
        rb.try_push(2);
        rb.try_push(3);
        assert_eq!(rb.len(), 3);

        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.pop(), None);
    }

    #[test]
    fn test_clear_empty_buffer() {
        let rb: RingBuffer<i32> = RingBuffer::new(16);
        rb.clear(); // Should not panic
        assert!(rb.is_empty());
    }

    // ==================== Drop Tests ====================

    #[test]
    fn test_drop_with_items() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct DropCounter;
        impl Drop for DropCounter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        DROP_COUNT.store(0, Ordering::Relaxed);

        {
            let rb = RingBuffer::new(16);
            rb.try_push(DropCounter);
            rb.try_push(DropCounter);
            rb.try_push(DropCounter);
            // rb drops here
        }

        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 3);
    }

    // ==================== Type Tests ====================

    #[test]
    fn test_with_strings() {
        let rb = RingBuffer::new(8);

        rb.try_push(String::from("hello"));
        rb.try_push(String::from("world"));

        assert_eq!(rb.pop(), Some(String::from("hello")));
        assert_eq!(rb.pop(), Some(String::from("world")));
    }

    #[test]
    fn test_with_buffer_entry() {
        let rb = RingBuffer::new(8);

        let entry = BufferEntry::new(0x0100, 1, 1234567890, vec![1, 2, 3, 4]);
        rb.try_push(entry);

        let popped = rb.pop().unwrap();
        assert_eq!(popped.record_type, 0x0100);
        assert_eq!(popped.exchange_id, 1);
        assert_eq!(popped.timestamp, 1234567890);
        assert_eq!(popped.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_with_zero_sized_type() {
        let rb = RingBuffer::<()>::new(8);

        rb.try_push(());
        rb.try_push(());

        assert_eq!(rb.pop(), Some(()));
        assert_eq!(rb.pop(), Some(()));
        assert_eq!(rb.pop(), None);
    }

    // ==================== Concurrent Tests ====================

    #[test]
    fn test_concurrent_spsc() {
        let rb = Arc::new(RingBuffer::new(1024));
        let rb_producer = Arc::clone(&rb);
        let rb_consumer = Arc::clone(&rb);

        const NUM_ITEMS: i32 = 10000;

        // Producer thread
        let producer = thread::spawn(move || {
            for i in 0..NUM_ITEMS {
                // Spin until we can push
                while !rb_producer.try_push(i) {
                    std::hint::spin_loop();
                }
            }
        });

        // Consumer thread
        let consumer = thread::spawn(move || {
            let mut received = Vec::with_capacity(NUM_ITEMS as usize);
            while received.len() < NUM_ITEMS as usize {
                if let Some(item) = rb_consumer.pop() {
                    received.push(item);
                } else {
                    std::hint::spin_loop();
                }
            }
            received
        });

        producer.join().unwrap();
        let received = consumer.join().unwrap();

        // Verify all items received in order
        assert_eq!(received.len(), NUM_ITEMS as usize);
        for (i, &item) in received.iter().enumerate() {
            assert_eq!(
                item, i as i32,
                "Item at index {} was {}, expected {}",
                i, item, i
            );
        }
    }

    #[test]
    fn test_concurrent_stress() {
        // Multiple rounds of concurrent testing
        for _ in 0..10 {
            let rb = Arc::new(RingBuffer::new(64));
            let rb_producer = Arc::clone(&rb);
            let rb_consumer = Arc::clone(&rb);

            const NUM_ITEMS: usize = 1000;

            let producer = thread::spawn(move || {
                for i in 0..NUM_ITEMS {
                    while !rb_producer.try_push(i) {
                        std::hint::spin_loop();
                    }
                }
            });

            let consumer = thread::spawn(move || {
                let mut sum = 0usize;
                let mut count = 0usize;
                while count < NUM_ITEMS {
                    if let Some(item) = rb_consumer.pop() {
                        sum += item;
                        count += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
                sum
            });

            producer.join().unwrap();
            let sum = consumer.join().unwrap();

            // Verify sum: 0 + 1 + 2 + ... + 999 = 999 * 1000 / 2 = 499500
            assert_eq!(sum, (NUM_ITEMS - 1) * NUM_ITEMS / 2);
        }
    }

    #[test]
    fn test_concurrent_with_small_buffer() {
        // Test with very small buffer to stress wrap-around
        let rb = Arc::new(RingBuffer::new(4));
        let rb_producer = Arc::clone(&rb);
        let rb_consumer = Arc::clone(&rb);

        const NUM_ITEMS: i32 = 1000;

        let producer = thread::spawn(move || {
            for i in 0..NUM_ITEMS {
                while !rb_producer.try_push(i) {
                    std::hint::spin_loop();
                }
            }
        });

        let consumer = thread::spawn(move || {
            let mut expected = 0;
            while expected < NUM_ITEMS {
                if let Some(item) = rb_consumer.pop() {
                    assert_eq!(
                        item, expected,
                        "Out of order: expected {}, got {}",
                        expected, item
                    );
                    expected += 1;
                } else {
                    std::hint::spin_loop();
                }
            }
        });

        producer.join().unwrap();
        consumer.join().unwrap();
    }

    // ==================== Edge Case Tests ====================

    #[test]
    fn test_pop_empty() {
        let rb: RingBuffer<i32> = RingBuffer::new(16);
        assert_eq!(rb.pop(), None);
        assert_eq!(rb.pop(), None);
    }

    #[test]
    fn test_push_after_wrap() {
        let rb = RingBuffer::new(4);

        // Fill completely
        rb.try_push(1);
        rb.try_push(2);
        rb.try_push(3);

        // Empty completely
        rb.pop();
        rb.pop();
        rb.pop();

        // Fill again (indices are now > 0)
        rb.try_push(4);
        rb.try_push(5);
        rb.try_push(6);

        assert_eq!(rb.pop(), Some(4));
        assert_eq!(rb.pop(), Some(5));
        assert_eq!(rb.pop(), Some(6));
    }

    // ==================== Memory Layout Tests ====================

    #[test]
    fn test_cache_line_padding() {
        // Verify that PaddedAtomicUsize is properly aligned
        assert_eq!(
            std::mem::align_of::<PaddedAtomicUsize>(),
            64,
            "PaddedAtomicUsize should be 64-byte aligned"
        );

        // Verify that head and tail don't share cache lines
        let rb = RingBuffer::<i32>::new(16);
        let head_addr = &rb.head as *const _ as usize;
        let tail_addr = &rb.tail as *const _ as usize;

        // Head and tail should be at least 64 bytes apart
        let distance = head_addr.abs_diff(tail_addr);

        assert!(
            distance >= 64,
            "Head and tail should be at least 64 bytes apart, actual distance: {}",
            distance
        );
    }
}
