//! Journal benchmarks.
//!
//! Run with: cargo bench --package blackbox
//!
//! ## Performance Targets
//!
//! Schema operations (build.rs embedding):
//! - `create_block` (compressed): < 1ms (one-time at journal creation)
//! - `create_block` (uncompressed): < 100μs
//! - `parse_block`: < 500μs (one-time at journal open)
//! - `compute_hash`: < 100μs
//! - `embedded_xml`: < 1ns (compile-time constant)
//!
//! RingBuffer operations (hot path):
//! - `try_push`: < 50ns (target: zero allocations)
//! - `pop`: < 50ns (target: zero allocations)
//! - Concurrent SPSC: maintains FIFO ordering

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;
use std::thread;

use blackbox::codec::{
    global_registry, CodecRegistry, Exchange, QuoteUpdateData, RawFrameData, Symbol,
};
use blackbox::journal::{
    schema, BufferEntry, JournalReader, JournalWriter, ReaderConfig, RecordType, RingBuffer,
    WriterConfig,
};

fn bench_schema_embedding(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_embedding");

    // Benchmark: Access embedded XML (should be instantaneous - compile-time constant)
    group.bench_function("embedded_xml", |b| {
        b.iter(|| black_box(schema::embedded_xml()))
    });

    // Benchmark: Access embedded hash
    group.bench_function("embedded_hash", |b| {
        b.iter(|| black_box(schema::embedded_hash()))
    });

    // Benchmark: Access embedded version
    group.bench_function("embedded_version", |b| {
        b.iter(|| black_box(schema::embedded_version()))
    });

    group.finish();
}

fn bench_schema_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_hash");

    let xml = schema::embedded_xml();
    let xml_bytes = xml.as_bytes();

    // Set throughput based on schema size
    group.throughput(Throughput::Bytes(xml_bytes.len() as u64));

    // Benchmark: Compute hash of schema
    group.bench_function("compute_hash", |b| {
        b.iter(|| black_box(schema::compute_hash(black_box(xml_bytes))))
    });

    // Benchmark: Verify hash
    let expected_hash = schema::embedded_hash();
    group.bench_function("verify_hash", |b| {
        b.iter(|| {
            black_box(schema::verify_hash(
                black_box(xml_bytes),
                black_box(expected_hash),
            ))
        })
    });

    group.finish();
}

fn bench_schema_block_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_block_create");

    // Benchmark: Create uncompressed block
    group.bench_function("uncompressed", |b| {
        b.iter(|| black_box(schema::create_block(false).expect("create block")))
    });

    // Benchmark: Create compressed block (level 3 - default)
    group.bench_function("compressed_level_3", |b| {
        b.iter(|| black_box(schema::create_block(true).expect("create block")))
    });

    // Benchmark: Create compressed block (level 1 - fast)
    group.bench_function("compressed_level_1", |b| {
        b.iter(|| black_box(schema::create_block_with_level(true, 1).expect("create block")))
    });

    // Benchmark: Create compressed block (level 9 - high)
    group.bench_function("compressed_level_9", |b| {
        b.iter(|| black_box(schema::create_block_with_level(true, 9).expect("create block")))
    });

    group.finish();
}

fn bench_schema_block_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_block_parse");

    // Pre-create blocks for parsing benchmarks
    let block_uncompressed = schema::create_block(false).expect("create block");
    let bytes_uncompressed = schema::serialize_block(&block_uncompressed);

    let block_compressed = schema::create_block(true).expect("create block");
    let bytes_compressed = schema::serialize_block(&block_compressed);

    // Set throughput based on compressed block size
    group.throughput(Throughput::Bytes(bytes_compressed.len() as u64));

    // Benchmark: Parse uncompressed block
    group.bench_function("uncompressed", |b| {
        b.iter(|| black_box(schema::parse_block(black_box(&bytes_uncompressed)).expect("parse")))
    });

    // Benchmark: Parse compressed block
    group.bench_function("compressed", |b| {
        b.iter(|| black_box(schema::parse_block(black_box(&bytes_compressed)).expect("parse")))
    });

    group.finish();
}

fn bench_schema_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_serialize");

    let block = schema::create_block(true).expect("create block");

    group.throughput(Throughput::Bytes(block.total_size() as u64));

    // Benchmark: Serialize block to bytes
    group.bench_function("serialize_block", |b| {
        b.iter(|| black_box(schema::serialize_block(black_box(&block))))
    });

    group.finish();
}

fn bench_journal_header_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal_header");

    // Benchmark: Create complete journal header with schema (compressed)
    group.bench_function("create_header_compressed", |b| {
        b.iter(|| black_box(schema::create_journal_header(true).expect("create header")))
    });

    // Benchmark: Create complete journal header with schema (uncompressed)
    group.bench_function("create_header_uncompressed", |b| {
        b.iter(|| black_box(schema::create_journal_header(false).expect("create header")))
    });

    group.finish();
}

// ==================== RingBuffer Benchmarks ====================

fn bench_ringbuffer_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("ringbuffer_push");

    // Benchmark: Push single item (hot path)
    group.bench_function("try_push_u64", |b| {
        let rb = RingBuffer::<u64>::new(1024);
        let mut i = 0u64;
        b.iter(|| {
            if !rb.try_push(black_box(i)) {
                // Buffer full, drain it
                while rb.pop().is_some() {}
            }
            i = i.wrapping_add(1);
        });
    });

    // Benchmark: Push with BufferEntry (realistic payload)
    group.bench_function("try_push_buffer_entry", |b| {
        let rb = RingBuffer::<BufferEntry>::new(1024);
        b.iter(|| {
            let entry = BufferEntry::new(0x0100, 1, 1234567890, vec![1, 2, 3, 4]);
            if !rb.try_push(black_box(entry)) {
                while rb.pop().is_some() {}
            }
        });
    });

    group.finish();
}

fn bench_ringbuffer_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("ringbuffer_pop");

    // Benchmark: Pop single item
    group.bench_function("pop_u64", |b| {
        let rb = RingBuffer::<u64>::new(1024);
        // Pre-fill buffer
        for i in 0..512 {
            rb.try_push(i);
        }
        let mut refill_counter = 0u64;
        b.iter(|| {
            if rb.pop().is_none() {
                // Buffer empty, refill it
                for i in 0..512 {
                    rb.try_push(refill_counter + i);
                }
                refill_counter += 512;
            }
        });
    });

    group.finish();
}

fn bench_ringbuffer_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("ringbuffer_throughput");

    // Set throughput based on number of operations
    const BATCH_SIZE: u64 = 1000;
    group.throughput(Throughput::Elements(BATCH_SIZE));

    // Benchmark: Push/pop cycle (sequential)
    group.bench_function("push_pop_cycle_1000", |b| {
        let rb = RingBuffer::<u64>::new(2048);
        b.iter(|| {
            // Push batch
            for i in 0..BATCH_SIZE {
                rb.try_push(black_box(i));
            }
            // Pop batch
            for _ in 0..BATCH_SIZE {
                black_box(rb.pop());
            }
        });
    });

    // Benchmark: Interleaved push/pop
    group.bench_function("interleaved_push_pop_1000", |b| {
        let rb = RingBuffer::<u64>::new(64);
        b.iter(|| {
            for i in 0..BATCH_SIZE {
                while !rb.try_push(black_box(i)) {
                    black_box(rb.pop());
                }
            }
            while rb.pop().is_some() {}
        });
    });

    group.finish();
}

fn bench_ringbuffer_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("ringbuffer_concurrent");

    const NUM_ITEMS: usize = 10_000;
    group.throughput(Throughput::Elements(NUM_ITEMS as u64));

    // Benchmark: Concurrent SPSC throughput
    group.bench_function("spsc_throughput_10000", |b| {
        b.iter(|| {
            let rb = Arc::new(RingBuffer::<u64>::new(1024));
            let rb_producer = Arc::clone(&rb);
            let rb_consumer = Arc::clone(&rb);

            let producer = thread::spawn(move || {
                for i in 0..NUM_ITEMS {
                    while !rb_producer.try_push(i as u64) {
                        std::hint::spin_loop();
                    }
                }
            });

            let consumer = thread::spawn(move || {
                let mut count = 0;
                while count < NUM_ITEMS {
                    if rb_consumer.pop().is_some() {
                        count += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
            });

            producer.join().unwrap();
            consumer.join().unwrap();
        });
    });

    // Benchmark: Concurrent with small buffer (high contention)
    group.bench_function("spsc_small_buffer_10000", |b| {
        b.iter(|| {
            let rb = Arc::new(RingBuffer::<u64>::new(8));
            let rb_producer = Arc::clone(&rb);
            let rb_consumer = Arc::clone(&rb);

            let producer = thread::spawn(move || {
                for i in 0..NUM_ITEMS {
                    while !rb_producer.try_push(i as u64) {
                        std::hint::spin_loop();
                    }
                }
            });

            let consumer = thread::spawn(move || {
                let mut count = 0;
                while count < NUM_ITEMS {
                    if rb_consumer.pop().is_some() {
                        count += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
            });

            producer.join().unwrap();
            consumer.join().unwrap();
        });
    });

    group.finish();
}

fn bench_ringbuffer_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("ringbuffer_capacity");

    // Benchmark: Status checks (is_empty, is_full, len)
    group.bench_function("is_empty", |b| {
        let rb = RingBuffer::<u64>::new(1024);
        b.iter(|| black_box(rb.is_empty()));
    });

    group.bench_function("is_full", |b| {
        let rb = RingBuffer::<u64>::new(1024);
        b.iter(|| black_box(rb.is_full()));
    });

    group.bench_function("len", |b| {
        let rb = RingBuffer::<u64>::new(1024);
        for i in 0..500 {
            rb.try_push(i);
        }
        b.iter(|| black_box(rb.len()));
    });

    group.finish();
}

// ==================== JournalWriter Benchmarks ====================

fn bench_writer_create(c: &mut Criterion) {
    let mut group = c.benchmark_group("writer_create");

    // Benchmark: Create new journal writer
    group.bench_function("new_writer", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            let path = std::env::temp_dir().join(format!("bench_journal_{}.journal", counter));
            counter += 1;
            let config = WriterConfig::minimal();
            let writer = JournalWriter::new(&path, config).expect("create writer");
            writer.close().expect("close writer");
            std::fs::remove_file(&path).ok();
        });
    });

    group.finish();
}

fn bench_writer_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("writer_write");

    // Create a persistent writer for write benchmarks
    let path = std::env::temp_dir().join("bench_write_hot_path.journal");
    let _ = std::fs::remove_file(&path);
    let config = WriterConfig {
        ring_buffer_capacity: 65536,
        file_size: 64 * 1024 * 1024,
        compress_schema: true,
        sync_on_close: false,
        prefault_pages: false,
    };
    let mut writer = JournalWriter::new(&path, config).expect("create writer");

    // Benchmark: Write single record (hot path)
    let payload_small = b"small payload";
    group.bench_function("write_small_payload", |b| {
        b.iter(|| {
            writer
                .write(
                    black_box(RecordType::RawFrame),
                    black_box(1),
                    black_box(payload_small),
                )
                .ok();
        });
    });

    // Benchmark: Write medium payload
    let payload_medium = vec![0x42u8; 256];
    group.bench_function("write_256b_payload", |b| {
        b.iter(|| {
            writer
                .write(
                    black_box(RecordType::RawFrame),
                    black_box(1),
                    black_box(&payload_medium),
                )
                .ok();
        });
    });

    // Benchmark: Write larger payload
    let payload_large = vec![0x42u8; 4096];
    group.bench_function("write_4kb_payload", |b| {
        b.iter(|| {
            writer
                .write(
                    black_box(RecordType::RawFrame),
                    black_box(1),
                    black_box(&payload_large),
                )
                .ok();
        });
    });

    drop(writer);
    std::fs::remove_file(&path).ok();
    group.finish();
}

fn bench_writer_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("writer_throughput");

    const BATCH_SIZE: u64 = 1000;
    group.throughput(Throughput::Elements(BATCH_SIZE));

    // Benchmark: Write many records
    group.bench_function("write_1000_records", |b| {
        let path = std::env::temp_dir().join("bench_throughput.journal");
        let _ = std::fs::remove_file(&path);
        let config = WriterConfig {
            ring_buffer_capacity: 16384,
            file_size: 64 * 1024 * 1024,
            compress_schema: true,
            sync_on_close: false,
            prefault_pages: false,
        };
        let mut writer = JournalWriter::new(&path, config).expect("create writer");
        let payload = b"throughput benchmark payload data";

        b.iter(|| {
            for _ in 0..BATCH_SIZE {
                writer
                    .write(
                        black_box(RecordType::RawFrame),
                        black_box(1),
                        black_box(payload),
                    )
                    .ok();
            }
        });

        drop(writer);
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

fn bench_writer_flush(c: &mut Criterion) {
    let mut group = c.benchmark_group("writer_flush");

    // Benchmark: Flush after writes
    group.bench_function("flush_100_records", |b| {
        let path = std::env::temp_dir().join("bench_flush.journal");
        let _ = std::fs::remove_file(&path);
        let config = WriterConfig {
            ring_buffer_capacity: 1024,
            file_size: 64 * 1024 * 1024,
            compress_schema: true,
            sync_on_close: false,
            prefault_pages: false,
        };
        let mut writer = JournalWriter::new(&path, config).expect("create writer");
        let payload = b"flush benchmark data";

        b.iter(|| {
            for _ in 0..100 {
                writer.write(RecordType::RawFrame, 1, payload).ok();
            }
            writer.flush().ok();
        });

        drop(writer);
        std::fs::remove_file(&path).ok();
    });

    group.finish();
}

// ==================== JournalReader Benchmarks ====================

fn bench_reader_open(c: &mut Criterion) {
    let mut group = c.benchmark_group("reader_open");

    // Create a test journal with records
    let path = std::env::temp_dir().join("bench_reader_open.journal");
    let _ = std::fs::remove_file(&path);
    let config = WriterConfig::minimal();
    let mut writer = JournalWriter::new(&path, config).expect("create writer");
    for i in 0..100 {
        writer
            .write(RecordType::RawFrame, 1, format!("payload {}", i).as_bytes())
            .ok();
    }
    writer.close().expect("close writer");

    // Benchmark: Open journal file
    group.bench_function("open_journal", |b| {
        b.iter(|| {
            let reader = JournalReader::open(black_box(&path)).expect("open reader");
            black_box(reader)
        });
    });

    // Benchmark: Open with fast config (no CRC, no schema hash verification)
    group.bench_function("open_journal_fast", |b| {
        b.iter(|| {
            let reader = JournalReader::open_with_config(black_box(&path), ReaderConfig::fast())
                .expect("open reader");
            black_box(reader)
        });
    });

    // Benchmark: Open with schema hash verification only (no CRC)
    group.bench_function("open_journal_schema_hash_only", |b| {
        let config = ReaderConfig {
            verify_crc: false,
            skip_corrupt: false,
            verify_schema_hash: true,
        };
        b.iter(|| {
            let reader = JournalReader::open_with_config(black_box(&path), config.clone())
                .expect("open reader");
            black_box(reader)
        });
    });

    // Benchmark: Open without schema hash verification (CRC enabled)
    group.bench_function("open_journal_skip_schema_hash", |b| {
        b.iter(|| {
            let reader =
                JournalReader::open_with_config(black_box(&path), ReaderConfig::skip_schema_hash())
                    .expect("open reader");
            black_box(reader)
        });
    });

    std::fs::remove_file(&path).ok();
    group.finish();
}

fn bench_reader_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("reader_read");

    // Create a test journal with records
    let path = std::env::temp_dir().join("bench_reader_read.journal");
    let _ = std::fs::remove_file(&path);
    let config = WriterConfig {
        ring_buffer_capacity: 4096,
        file_size: 4 * 1024 * 1024,
        compress_schema: true,
        sync_on_close: false,
        prefault_pages: false,
    };
    let mut writer = JournalWriter::new(&path, config).expect("create writer");
    let payload = b"benchmark record payload data";
    for _ in 0..1000 {
        writer.write(RecordType::RawFrame, 1, payload).ok();
    }
    writer.close().expect("close writer");

    // Benchmark: Read single record (with CRC verification)
    group.bench_function("read_record_with_crc", |b| {
        let mut reader = JournalReader::open(&path).expect("open reader");
        b.iter(|| {
            if reader.next().is_none() {
                reader.rewind();
            }
        });
    });

    // Benchmark: Read single record (without CRC verification)
    group.bench_function("read_record_no_crc", |b| {
        let mut reader =
            JournalReader::open_with_config(&path, ReaderConfig::fast()).expect("open reader");
        b.iter(|| {
            if reader.next().is_none() {
                reader.rewind();
            }
        });
    });

    std::fs::remove_file(&path).ok();
    group.finish();
}

fn bench_reader_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("reader_throughput");

    const NUM_RECORDS: u64 = 1000;
    group.throughput(Throughput::Elements(NUM_RECORDS));

    // Create a test journal with many records
    let path = std::env::temp_dir().join("bench_reader_throughput.journal");
    let _ = std::fs::remove_file(&path);
    let config = WriterConfig {
        ring_buffer_capacity: 4096,
        file_size: 8 * 1024 * 1024,
        compress_schema: true,
        sync_on_close: false,
        prefault_pages: false,
    };
    let mut writer = JournalWriter::new(&path, config).expect("create writer");
    let payload = b"benchmark record payload data for throughput test";
    for _ in 0..NUM_RECORDS {
        writer.write(RecordType::RawFrame, 1, payload).ok();
    }
    writer.close().expect("close writer");

    // Benchmark: Read 1000 records
    group.bench_function("read_1000_records", |b| {
        b.iter(|| {
            let reader = JournalReader::open(&path).expect("open reader");
            for record in reader {
                black_box(record.ok());
            }
        });
    });

    // Benchmark: Read 1000 records (fast mode, no CRC)
    group.bench_function("read_1000_records_fast", |b| {
        b.iter(|| {
            let reader =
                JournalReader::open_with_config(&path, ReaderConfig::fast()).expect("open reader");
            for record in reader {
                black_box(record.ok());
            }
        });
    });

    std::fs::remove_file(&path).ok();
    group.finish();
}

fn bench_reader_rewind(c: &mut Criterion) {
    let mut group = c.benchmark_group("reader_rewind");

    // Create a test journal with records
    let path = std::env::temp_dir().join("bench_reader_rewind.journal");
    let _ = std::fs::remove_file(&path);
    let config = WriterConfig::minimal();
    let mut writer = JournalWriter::new(&path, config).expect("create writer");
    for i in 0..100 {
        writer
            .write(RecordType::RawFrame, 1, format!("payload {}", i).as_bytes())
            .ok();
    }
    writer.close().expect("close writer");

    // Benchmark: Rewind operation
    let mut reader = JournalReader::open(&path).expect("open reader");
    // Read to end first
    while reader.next().is_some() {}

    group.bench_function("rewind", |b| {
        b.iter(|| {
            reader.rewind();
            black_box(reader.position())
        });
    });

    std::fs::remove_file(&path).ok();
    group.finish();
}

// ==================== Codec Benchmarks ====================

fn bench_codec_registry(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_registry");

    // Benchmark: Create new registry
    group.bench_function("new_registry", |b| {
        b.iter(|| black_box(CodecRegistry::new()))
    });

    // Benchmark: Get global registry (cached)
    group.bench_function("global_registry", |b| {
        b.iter(|| black_box(global_registry()))
    });

    // Benchmark: Get decoder for version
    let registry = CodecRegistry::new();
    group.bench_function("get_decoder", |b| {
        b.iter(|| black_box(registry.decoder(black_box(1), black_box(0))))
    });

    // Benchmark: Get decoder with fallback
    group.bench_function("get_decoder_fallback", |b| {
        b.iter(|| black_box(registry.decoder_with_fallback(black_box(1), black_box(5))))
    });

    group.finish();
}

fn bench_codec_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_encode");

    let registry = CodecRegistry::new();
    let encoder = registry.encoder();

    // Prepare test data
    let raw_frame = RawFrameData {
        timestamp: 1_704_067_200_000_000,
        exchange: Exchange::Deribit,
        sequence_number: 42,
        payload: b"test websocket payload data".to_vec(),
    };

    let quote_update = QuoteUpdateData {
        timestamp: 1_704_067_200_000_000,
        exchange: Exchange::Binance,
        symbol: Symbol::new("BTC-USDT"),
        bid_price: 50000_00000000,
        bid_size: 1_50000000,
        ask_price: 50001_00000000,
        ask_size: 2_25000000,
    };

    // Benchmark: Encode RawFrame
    let mut buf = vec![0u8; 256];
    group.bench_function("encode_raw_frame", |b| {
        b.iter(|| {
            black_box(
                encoder
                    .encode_raw_frame(black_box(&raw_frame), &mut buf)
                    .unwrap(),
            )
        })
    });

    // Benchmark: Encode QuoteUpdate
    group.bench_function("encode_quote_update", |b| {
        b.iter(|| {
            black_box(
                encoder
                    .encode_quote_update(black_box(&quote_update), &mut buf)
                    .unwrap(),
            )
        })
    });

    group.finish();
}

fn bench_codec_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_decode");

    let registry = CodecRegistry::new();
    let encoder = registry.encoder();
    let decoder = registry.decoder(1, 0).unwrap();

    // Prepare encoded test data
    let raw_frame = RawFrameData {
        timestamp: 1_704_067_200_000_000,
        exchange: Exchange::Deribit,
        sequence_number: 42,
        payload: b"test websocket payload data".to_vec(),
    };
    let mut raw_frame_buf = vec![0u8; 256];
    let raw_frame_size = encoder
        .encode_raw_frame(&raw_frame, &mut raw_frame_buf)
        .unwrap();

    let quote_update = QuoteUpdateData {
        timestamp: 1_704_067_200_000_000,
        exchange: Exchange::Binance,
        symbol: Symbol::new("BTC-USDT"),
        bid_price: 50000_00000000,
        bid_size: 1_50000000,
        ask_price: 50001_00000000,
        ask_size: 2_25000000,
    };
    let mut quote_buf = vec![0u8; 128];
    let quote_size = encoder
        .encode_quote_update(&quote_update, &mut quote_buf)
        .unwrap();

    // Benchmark: Decode RawFrame
    group.bench_function("decode_raw_frame", |b| {
        b.iter(|| {
            black_box(
                decoder
                    .decode_raw_frame(black_box(&raw_frame_buf[..raw_frame_size]))
                    .unwrap(),
            )
        })
    });

    // Benchmark: Decode QuoteUpdate
    group.bench_function("decode_quote_update", |b| {
        b.iter(|| {
            black_box(
                decoder
                    .decode_quote_update(black_box(&quote_buf[..quote_size]))
                    .unwrap(),
            )
        })
    });

    group.finish();
}

fn bench_codec_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_roundtrip");

    let registry = CodecRegistry::new();
    let encoder = registry.encoder();
    let decoder = registry.decoder(1, 0).unwrap();

    // Prepare test data
    let raw_frame = RawFrameData {
        timestamp: 1_704_067_200_000_000,
        exchange: Exchange::Deribit,
        sequence_number: 42,
        payload: b"test websocket payload data for roundtrip benchmark".to_vec(),
    };

    let mut buf = vec![0u8; 256];

    // Benchmark: Full encode-decode roundtrip
    group.bench_function("raw_frame_roundtrip", |b| {
        b.iter(|| {
            let size = encoder.encode_raw_frame(&raw_frame, &mut buf).unwrap();
            black_box(decoder.decode_raw_frame(&buf[..size]).unwrap())
        })
    });

    group.finish();
}

fn bench_codec_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("codec_throughput");

    const BATCH_SIZE: u64 = 1000;
    group.throughput(Throughput::Elements(BATCH_SIZE));

    let registry = CodecRegistry::new();
    let encoder = registry.encoder();
    let decoder = registry.decoder(1, 0).unwrap();

    // Prepare encoded messages
    let mut encoded_messages = Vec::with_capacity(BATCH_SIZE as usize);
    for i in 0..BATCH_SIZE {
        let raw_frame = RawFrameData {
            timestamp: 1_704_067_200_000_000 + i as i64,
            exchange: Exchange::Deribit,
            sequence_number: i as u32,
            payload: format!("payload {}", i).into_bytes(),
        };
        let mut buf = vec![0u8; 128];
        let size = encoder.encode_raw_frame(&raw_frame, &mut buf).unwrap();
        buf.truncate(size);
        encoded_messages.push(buf);
    }

    // Benchmark: Decode 1000 messages
    group.bench_function("decode_1000_messages", |b| {
        b.iter(|| {
            for msg in &encoded_messages {
                black_box(decoder.decode_raw_frame(msg).unwrap());
            }
        })
    });

    group.finish();
}

// ==================== Tap Benchmarks ====================

fn bench_tap_null(c: &mut Criterion) {
    use blackbox::tap::{NullTap, Tap};
    use blackbox_types::{Exchange, Timestamp};

    let mut group = c.benchmark_group("tap_null");

    let tap = NullTap;
    let ts = Timestamp::from_micros(1_704_067_200_000_000);
    let payload = b"test payload data for benchmarking";

    // Benchmark: NullTap record_ingress (should be ~0ns - fully inlined no-op)
    group.bench_function("record_ingress", |b| {
        b.iter(|| {
            black_box(&tap).record_ingress(
                black_box(Exchange::Deribit),
                black_box(payload),
                black_box(ts),
            )
        })
    });

    // Benchmark: NullTap record_egress
    group.bench_function("record_egress", |b| {
        b.iter(|| {
            black_box(&tap).record_egress(
                black_box(Exchange::Binance),
                black_box(payload),
                black_box(ts),
            )
        })
    });

    // Benchmark: NullTap record_internal
    group.bench_function("record_internal", |b| {
        b.iter(|| {
            black_box(&tap).record_internal(black_box(0x0010), black_box(payload), black_box(ts))
        })
    });

    // Benchmark: NullTap record_checkpoint
    let hash = [0u8; 32];
    group.bench_function("record_checkpoint", |b| {
        b.iter(|| black_box(&tap).record_checkpoint(black_box(&hash), black_box(ts)))
    });

    // Benchmark: NullTap is_active
    group.bench_function("is_active", |b| b.iter(|| black_box(tap.is_active())));

    group.finish();
}

fn bench_tap_journal(c: &mut Criterion) {
    use blackbox::journal::WriterConfig;
    use blackbox::tap::{JournalTap, Tap};
    use blackbox_types::{Exchange, Timestamp};
    use std::sync::Arc;

    let mut group = c.benchmark_group("tap_journal");
    group.sample_size(100); // Reduce sample size due to file I/O

    let ts = Timestamp::from_micros(1_704_067_200_000_000);
    let payload = b"test payload data for benchmarking";

    // Create a temp directory for journal files
    let temp_dir = std::env::temp_dir();

    // Benchmark: JournalTap record_ingress
    group.bench_function("record_ingress", |b| {
        // Create fresh tap for each iteration group
        let path = temp_dir.join(format!("bench_tap_ingress_{}.journal", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let config = WriterConfig::minimal();
        let writer = blackbox::journal::JournalWriter::new(&path, config).unwrap();
        let tap = Arc::new(JournalTap::new(writer));

        b.iter(|| {
            tap.record_ingress(
                black_box(Exchange::Deribit),
                black_box(payload),
                black_box(ts),
            )
        });

        let _ = std::fs::remove_file(&path);
    });

    // Benchmark: JournalTap is_active
    group.bench_function("is_active", |b| {
        let path = temp_dir.join(format!("bench_tap_active_{}.journal", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let config = WriterConfig::minimal();
        let writer = blackbox::journal::JournalWriter::new(&path, config).unwrap();
        let tap = JournalTap::new(writer);

        b.iter(|| black_box(tap.is_active()));

        let _ = std::fs::remove_file(&path);
    });

    group.finish();
}

fn bench_tap_generic(c: &mut Criterion) {
    use blackbox::tap::{NullTap, Tap};
    use blackbox_types::{Exchange, Timestamp};

    let mut group = c.benchmark_group("tap_generic");

    // Benchmark: Generic function with NullTap (verifies zero overhead)
    fn process_event<T: Tap>(tap: &T, exchange: Exchange, payload: &[u8], ts: Timestamp) {
        if tap.is_active() {
            tap.record_ingress(exchange, payload, ts);
        }
    }

    let tap = NullTap;
    let ts = Timestamp::from_micros(1_704_067_200_000_000);
    let payload = b"test payload data for benchmarking";

    group.bench_function("generic_null_tap", |b| {
        b.iter(|| {
            process_event(
                black_box(&tap),
                black_box(Exchange::Deribit),
                black_box(payload),
                black_box(ts),
            )
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_schema_embedding,
    bench_schema_hash,
    bench_schema_block_creation,
    bench_schema_block_parsing,
    bench_schema_serialization,
    bench_journal_header_creation,
    bench_ringbuffer_push,
    bench_ringbuffer_pop,
    bench_ringbuffer_throughput,
    bench_ringbuffer_concurrent,
    bench_ringbuffer_capacity,
    bench_writer_create,
    bench_writer_write,
    bench_writer_throughput,
    bench_writer_flush,
    bench_reader_open,
    bench_reader_read,
    bench_reader_throughput,
    bench_reader_rewind,
    bench_codec_registry,
    bench_codec_encode,
    bench_codec_decode,
    bench_codec_roundtrip,
    bench_codec_throughput,
    bench_tap_null,
    bench_tap_journal,
    bench_tap_generic,
);
criterion_main!(benches);
