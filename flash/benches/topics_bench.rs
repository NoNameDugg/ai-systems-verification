//! Benchmarks for the Topic Routing module.
//!
//! Run with: `cargo bench --bench topics_bench`
//!
//! # Performance Budgets
//!
//! | Operation | Target |
//! |-----------|--------|
//! | Pattern match | < 100 ns |
//! | Filter evaluation | < 50 ns |
//! | Template expansion | < 200 ns |
//! | Full routing (1 rule) | < 500 ns |
//! | Full routing (10 rules) | < 2 μs |

use astra_flash::core::types::{Exchange, Instrument, MarketData, MarketEvent, MarketEventType};
use astra_flash::publisher::stream::TopicType;
use astra_flash::publisher::topics::{
    RouterConfig, RouterConfigBuilder, RoutingAction, RoutingFilter, RoutingFilterBuilder,
    RoutingResult, RoutingRule, RoutingRuleBuilder, RoutingStats, RuleId, TopicPattern,
    TopicRouter, TopicRouterBuilder, TopicTemplate,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test MarketEvent with order book snapshot.
fn test_book_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument: Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
        timestamp: 1700000000000,
        local_timestamp: 1700000000001,
        sequence: Some(12345),
        data: MarketData::Book {
            bids: vec![],
            asks: vec![],
        },
    }
}

/// Create a test MarketEvent for a specific exchange.
fn test_event_for_exchange(exchange: Exchange) -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument: Instrument::new("ETH", "USDT", exchange, "ETHUSDT"),
        timestamp: 1700000000000,
        local_timestamp: 1700000000001,
        sequence: None,
        data: MarketData::Book {
            bids: vec![],
            asks: vec![],
        },
    }
}

// =============================================================================
// TOPIC PATTERN BENCHMARKS
// =============================================================================

/// Benchmark TopicPattern parsing - exact pattern.
fn bench_pattern_parse_exact(c: &mut Criterion) {
    c.bench_function("TopicPattern::parse_exact", |b| {
        b.iter(|| {
            let pattern = TopicPattern::parse(black_box("market_data.deribit.btc_usd.book"));
            black_box(pattern)
        });
    });
}

/// Benchmark TopicPattern parsing - with wildcards.
fn bench_pattern_parse_wildcard(c: &mut Criterion) {
    c.bench_function("TopicPattern::parse_wildcard", |b| {
        b.iter(|| {
            let pattern = TopicPattern::parse(black_box("market_data.*.btc_usd.#"));
            black_box(pattern)
        });
    });
}

/// Benchmark TopicPattern matching - exact match.
fn bench_pattern_match_exact(c: &mut Criterion) {
    let pattern = TopicPattern::parse("market_data.deribit.btc_usd.book").unwrap();
    let topic = "market_data.deribit.btc_usd.book";

    c.bench_function("TopicPattern::matches_exact", |b| {
        b.iter(|| {
            let result = pattern.matches(black_box(topic));
            black_box(result)
        });
    });
}

/// Benchmark TopicPattern matching - single wildcard.
fn bench_pattern_match_single_wildcard(c: &mut Criterion) {
    let pattern = TopicPattern::parse("market_data.*.btc_usd.book").unwrap();
    let topic = "market_data.deribit.btc_usd.book";

    c.bench_function("TopicPattern::matches_single_wildcard", |b| {
        b.iter(|| {
            let result = pattern.matches(black_box(topic));
            black_box(result)
        });
    });
}

/// Benchmark TopicPattern matching - multi-level wildcard.
fn bench_pattern_match_multi_wildcard(c: &mut Criterion) {
    let pattern = TopicPattern::parse("market_data.#").unwrap();
    let topic = "market_data.deribit.btc_usd.book";

    c.bench_function("TopicPattern::matches_multi_wildcard", |b| {
        b.iter(|| {
            let result = pattern.matches(black_box(topic));
            black_box(result)
        });
    });
}

/// Benchmark TopicPattern matching - multiple wildcards.
fn bench_pattern_match_multiple_wildcards(c: &mut Criterion) {
    let pattern = TopicPattern::parse("market_data.*.*.#").unwrap();
    let topic = "market_data.deribit.btc_usd.book.depth10";

    c.bench_function("TopicPattern::matches_multiple_wildcards", |b| {
        b.iter(|| {
            let result = pattern.matches(black_box(topic));
            black_box(result)
        });
    });
}

/// Benchmark TopicPattern cloning.
fn bench_pattern_clone(c: &mut Criterion) {
    let pattern = TopicPattern::parse("market_data.*.btc_usd.#").unwrap();

    c.bench_function("TopicPattern::clone", |b| {
        b.iter(|| {
            let cloned = pattern.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// TOPIC TEMPLATE BENCHMARKS
// =============================================================================

/// Benchmark TopicTemplate parsing - simple.
fn bench_template_parse_simple(c: &mut Criterion) {
    c.bench_function("TopicTemplate::parse_simple", |b| {
        b.iter(|| {
            let template = TopicTemplate::parse(black_box("market_data.deribit.btc_usd.book"));
            black_box(template)
        });
    });
}

/// Benchmark TopicTemplate parsing - with placeholders.
fn bench_template_parse_placeholders(c: &mut Criterion) {
    c.bench_function("TopicTemplate::parse_placeholders", |b| {
        b.iter(|| {
            let template =
                TopicTemplate::parse(black_box("{prefix}.{exchange}.{base}_{quote}.{type}"));
            black_box(template)
        });
    });
}

/// Benchmark TopicTemplate expansion.
fn bench_template_expand(c: &mut Criterion) {
    let template = TopicTemplate::parse("{prefix}.{exchange}.{base}_{quote}.{type}").unwrap();
    let event = test_book_event();

    c.bench_function("TopicTemplate::expand", |b| {
        b.iter(|| {
            let result = template.expand(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark TopicTemplate expansion - literal only.
fn bench_template_expand_literal(c: &mut Criterion) {
    let template = TopicTemplate::parse("market_data.deribit.btc_usd.book").unwrap();
    let event = test_book_event();

    c.bench_function("TopicTemplate::expand_literal", |b| {
        b.iter(|| {
            let result = template.expand(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark TopicTemplate cloning.
fn bench_template_clone(c: &mut Criterion) {
    let template = TopicTemplate::parse("{prefix}.{exchange}.{base}_{quote}.{type}").unwrap();

    c.bench_function("TopicTemplate::clone", |b| {
        b.iter(|| {
            let cloned = template.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// ROUTING FILTER BENCHMARKS
// =============================================================================

/// Benchmark RoutingFilter creation with builder.
fn bench_filter_builder(c: &mut Criterion) {
    c.bench_function("RoutingFilterBuilder::build", |b| {
        b.iter(|| {
            let filter = RoutingFilterBuilder::new()
                .exchange(Exchange::Deribit)
                .topic_type(TopicType::Book)
                .base_asset("BTC")
                .build();
            black_box(filter)
        });
    });
}

/// Benchmark RoutingFilter matching - matches.
fn bench_filter_matches_true(c: &mut Criterion) {
    let filter = RoutingFilterBuilder::new()
        .exchange(Exchange::Deribit)
        .topic_type(TopicType::Book)
        .build();
    let event = test_book_event();

    c.bench_function("RoutingFilter::matches_true", |b| {
        b.iter(|| {
            let result = filter.matches(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark RoutingFilter matching - doesn't match.
fn bench_filter_matches_false(c: &mut Criterion) {
    let filter = RoutingFilterBuilder::new()
        .exchange(Exchange::Binance)
        .topic_type(TopicType::Trade)
        .build();
    let event = test_book_event();

    c.bench_function("RoutingFilter::matches_false", |b| {
        b.iter(|| {
            let result = filter.matches(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark RoutingFilter matching - permissive filter.
fn bench_filter_matches_permissive(c: &mut Criterion) {
    let filter = RoutingFilter::default(); // Matches everything
    let event = test_book_event();

    c.bench_function("RoutingFilter::matches_permissive", |b| {
        b.iter(|| {
            let result = filter.matches(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark RoutingFilter cloning.
fn bench_filter_clone(c: &mut Criterion) {
    let filter = RoutingFilterBuilder::new()
        .exchange(Exchange::Deribit)
        .topic_type(TopicType::Book)
        .base_asset("BTC")
        .quote_asset("USD")
        .build();

    c.bench_function("RoutingFilter::clone", |b| {
        b.iter(|| {
            let cloned = filter.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// ROUTING RULE BENCHMARKS
// =============================================================================

/// Benchmark RoutingRule creation with builder.
fn bench_rule_builder(c: &mut Criterion) {
    c.bench_function("RoutingRuleBuilder::build", |b| {
        b.iter(|| {
            let rule = RoutingRuleBuilder::new("test_rule")
                .priority(10)
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                .action(RoutingAction::Route)
                .build();
            black_box(rule)
        });
    });
}

/// Benchmark RoutingRule cloning.
fn bench_rule_clone(c: &mut Criterion) {
    let rule = RoutingRuleBuilder::new("test_rule")
        .priority(10)
        .filter(
            RoutingFilterBuilder::new()
                .exchange(Exchange::Deribit)
                .build(),
        )
        .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
        .action(RoutingAction::Route)
        .build()
        .unwrap();

    c.bench_function("RoutingRule::clone", |b| {
        b.iter(|| {
            let cloned = rule.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// ROUTER CONFIG BENCHMARKS
// =============================================================================

/// Benchmark RouterConfig default creation.
fn bench_router_config_default(c: &mut Criterion) {
    c.bench_function("RouterConfig::default", |b| {
        b.iter(|| {
            let config = RouterConfig::default();
            black_box(config)
        });
    });
}

/// Benchmark RouterConfig builder.
fn bench_router_config_builder(c: &mut Criterion) {
    c.bench_function("RouterConfigBuilder::build", |b| {
        b.iter(|| {
            let config = RouterConfigBuilder::new()
                .default_prefix("market_data")
                .track_stats(true)
                .build();
            black_box(config)
        });
    });
}

/// Benchmark RouterConfig cloning.
fn bench_router_config_clone(c: &mut Criterion) {
    let config = RouterConfigBuilder::new()
        .default_prefix("market_data")
        .track_stats(true)
        .build();

    c.bench_function("RouterConfig::clone", |b| {
        b.iter(|| {
            let cloned = config.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// TOPIC ROUTER BENCHMARKS
// =============================================================================

/// Benchmark TopicRouter creation with no rules.
fn bench_router_new_empty(c: &mut Criterion) {
    let config = RouterConfig::default();

    c.bench_function("TopicRouter::new_empty", |b| {
        b.iter(|| {
            let router = TopicRouter::new(black_box(config.clone()));
            black_box(router)
        });
    });
}

/// Benchmark TopicRouter creation with builder.
fn bench_router_builder(c: &mut Criterion) {
    c.bench_function("TopicRouterBuilder::build", |b| {
        b.iter(|| {
            let router = TopicRouterBuilder::new()
                .config(RouterConfig::default())
                .add_rule(
                    RoutingRuleBuilder::new("deribit_rule")
                        .filter(
                            RoutingFilterBuilder::new()
                                .exchange(Exchange::Deribit)
                                .build(),
                        )
                        .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                        .build()
                        .unwrap(),
                )
                .build();
            black_box(router)
        });
    });
}

/// Benchmark TopicRouter routing - default (no rules match).
fn bench_router_route_default(c: &mut Criterion) {
    let router = TopicRouter::new(RouterConfig::default());
    let event = test_book_event();

    c.bench_function("TopicRouter::route_default", |b| {
        b.iter(|| {
            let result = router.route(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark TopicRouter routing - single rule match.
fn bench_router_route_single_rule(c: &mut Criterion) {
    let router = TopicRouterBuilder::new()
        .config(RouterConfig::default())
        .add_rule(
            RoutingRuleBuilder::new("deribit_rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                .action(RoutingAction::RouteAndStop)
                .build()
                .unwrap(),
        )
        .build();
    let event = test_book_event();

    c.bench_function("TopicRouter::route_single_rule", |b| {
        b.iter(|| {
            let result = router.route(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark TopicRouter routing - multiple rules (varying counts).
fn bench_router_route_multiple_rules(c: &mut Criterion) {
    let mut group = c.benchmark_group("TopicRouter::route_multiple_rules");

    for rule_count in [1, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(rule_count),
            rule_count,
            |b, &count| {
                let mut builder = TopicRouterBuilder::new().config(RouterConfig::default());

                // Add rules that DON'T match first (worst case)
                for i in 0..(count - 1) {
                    builder = builder.add_rule(
                        RoutingRuleBuilder::new(format!("rule_{}", i))
                            .priority(i as u32)
                            .filter(
                                RoutingFilterBuilder::new()
                                    .exchange(Exchange::Binance) // Won't match Deribit event
                                    .build(),
                            )
                            .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                            .build()
                            .unwrap(),
                    );
                }

                // Add one rule that matches at the end
                builder = builder.add_rule(
                    RoutingRuleBuilder::new("matching_rule")
                        .priority(count as u32)
                        .filter(
                            RoutingFilterBuilder::new()
                                .exchange(Exchange::Deribit)
                                .build(),
                        )
                        .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                        .action(RoutingAction::RouteAndStop)
                        .build()
                        .unwrap(),
                );

                let router = builder.build();
                let event = test_book_event();

                b.iter(|| {
                    let result = router.route(black_box(&event));
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark TopicRouter routing - rule with drop action.
fn bench_router_route_drop(c: &mut Criterion) {
    let router = TopicRouterBuilder::new()
        .config(RouterConfig::default())
        .add_rule(
            RoutingRuleBuilder::new("drop_rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .action(RoutingAction::Drop)
                .build()
                .unwrap(),
        )
        .build();
    let event = test_book_event();

    c.bench_function("TopicRouter::route_drop", |b| {
        b.iter(|| {
            let result = router.route(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark TopicRouter routing - multiple topics generated.
fn bench_router_route_multi_topic(c: &mut Criterion) {
    let router = TopicRouterBuilder::new()
        .config(RouterConfig::default())
        .add_rule(
            RoutingRuleBuilder::new("primary")
                .priority(1)
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("primary.{exchange}.{base}_{quote}")
                .action(RoutingAction::Route) // Continue to next rule
                .build()
                .unwrap(),
        )
        .add_rule(
            RoutingRuleBuilder::new("secondary")
                .priority(2)
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("secondary.{exchange}.{base}_{quote}")
                .action(RoutingAction::Route)
                .build()
                .unwrap(),
        )
        .add_rule(
            RoutingRuleBuilder::new("tertiary")
                .priority(3)
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("tertiary.{exchange}.{base}_{quote}")
                .action(RoutingAction::RouteAndStop)
                .build()
                .unwrap(),
        )
        .build();
    let event = test_book_event();

    c.bench_function("TopicRouter::route_multi_topic", |b| {
        b.iter(|| {
            let result = router.route(black_box(&event));
            black_box(result)
        });
    });
}

/// Benchmark TopicRouter add_rule.
fn bench_router_add_rule(c: &mut Criterion) {
    c.bench_function("TopicRouter::add_rule", |b| {
        b.iter(|| {
            let mut router = TopicRouter::new(RouterConfig::default());
            router.add_rule(
                RoutingRuleBuilder::new("test_rule")
                    .filter(
                        RoutingFilterBuilder::new()
                            .exchange(Exchange::Deribit)
                            .build(),
                    )
                    .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                    .build()
                    .unwrap(),
            );
            black_box(router)
        });
    });
}

/// Benchmark TopicRouter stats retrieval.
fn bench_router_stats(c: &mut Criterion) {
    let router = TopicRouterBuilder::new()
        .config(RouterConfigBuilder::new().track_stats(true).build())
        .add_rule(
            RoutingRuleBuilder::new("test_rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                .build()
                .unwrap(),
        )
        .build();

    // Route some events to populate stats
    let event = test_book_event();
    for _ in 0..100 {
        let _ = router.route(&event);
    }

    c.bench_function("TopicRouter::stats", |b| {
        b.iter(|| {
            let stats = router.stats();
            black_box(stats)
        });
    });
}

// =============================================================================
// ROUTING STATS BENCHMARKS
// =============================================================================

/// Benchmark RoutingStats default creation.
fn bench_routing_stats_default(c: &mut Criterion) {
    c.bench_function("RoutingStats::default", |b| {
        b.iter(|| {
            let stats = RoutingStats::default();
            black_box(stats)
        });
    });
}

/// Benchmark RoutingStats cloning.
fn bench_routing_stats_clone(c: &mut Criterion) {
    let stats = RoutingStats::default();

    c.bench_function("RoutingStats::clone", |b| {
        b.iter(|| {
            let cloned = stats.clone();
            black_box(cloned)
        });
    });
}

// =============================================================================
// RULE ID BENCHMARKS
// =============================================================================

/// Benchmark RuleId generation.
fn bench_rule_id_new(c: &mut Criterion) {
    c.bench_function("RuleId::new", |b| {
        b.iter(|| {
            let id = RuleId::new();
            black_box(id)
        });
    });
}

/// Benchmark RuleId comparison.
fn bench_rule_id_eq(c: &mut Criterion) {
    let id1 = RuleId::new();
    let id2 = RuleId::new();

    c.bench_function("RuleId::eq", |b| {
        b.iter(|| {
            let eq = id1 == black_box(id2);
            black_box(eq)
        });
    });
}

// =============================================================================
// TYPE SIZE BENCHMARKS
// =============================================================================

/// Benchmark memory sizes of types.
fn bench_type_sizes(c: &mut Criterion) {
    use std::mem::size_of;

    c.bench_function("topic_routing_type_sizes", |b| {
        b.iter(|| {
            let pattern_size = size_of::<TopicPattern>();
            let template_size = size_of::<TopicTemplate>();
            let filter_size = size_of::<RoutingFilter>();
            let rule_size = size_of::<RoutingRule>();
            let config_size = size_of::<RouterConfig>();
            let router_size = size_of::<TopicRouter>();
            let stats_size = size_of::<RoutingStats>();
            let result_size = size_of::<RoutingResult>();

            black_box((
                pattern_size,
                template_size,
                filter_size,
                rule_size,
                config_size,
                router_size,
                stats_size,
                result_size,
            ))
        });
    });

    // Print sizes for reference
    println!("\nTopic Routing Type Sizes:");
    println!(
        "  TopicPattern: {} bytes",
        std::mem::size_of::<TopicPattern>()
    );
    println!(
        "  TopicTemplate: {} bytes",
        std::mem::size_of::<TopicTemplate>()
    );
    println!(
        "  RoutingFilter: {} bytes",
        std::mem::size_of::<RoutingFilter>()
    );
    println!(
        "  RoutingRule: {} bytes",
        std::mem::size_of::<RoutingRule>()
    );
    println!(
        "  RouterConfig: {} bytes",
        std::mem::size_of::<RouterConfig>()
    );
    println!(
        "  TopicRouter: {} bytes",
        std::mem::size_of::<TopicRouter>()
    );
    println!(
        "  RoutingStats: {} bytes",
        std::mem::size_of::<RoutingStats>()
    );
    println!(
        "  RoutingResult: {} bytes",
        std::mem::size_of::<RoutingResult>()
    );
}

// =============================================================================
// THROUGHPUT BENCHMARKS
// =============================================================================

/// Benchmark routing throughput - messages per second.
fn bench_routing_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("routing_throughput");
    group.throughput(Throughput::Elements(1));

    let router = TopicRouterBuilder::new()
        .config(
            RouterConfigBuilder::new()
                .track_stats(false) // Disable stats for max throughput
                .build(),
        )
        .add_rule(
            RoutingRuleBuilder::new("fast_route")
                .filter(
                    RoutingFilterBuilder::new()
                        .exchange(Exchange::Deribit)
                        .build(),
                )
                .target_template("{prefix}.{exchange}.{base}_{quote}.{type}")
                .action(RoutingAction::RouteAndStop)
                .build()
                .unwrap(),
        )
        .build();

    let event = test_book_event();

    group.bench_function("route_single", |b| {
        b.iter(|| {
            let result = router.route(black_box(&event));
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    pattern_benchmarks,
    bench_pattern_parse_exact,
    bench_pattern_parse_wildcard,
    bench_pattern_match_exact,
    bench_pattern_match_single_wildcard,
    bench_pattern_match_multi_wildcard,
    bench_pattern_match_multiple_wildcards,
    bench_pattern_clone,
);

criterion_group!(
    template_benchmarks,
    bench_template_parse_simple,
    bench_template_parse_placeholders,
    bench_template_expand,
    bench_template_expand_literal,
    bench_template_clone,
);

criterion_group!(
    filter_benchmarks,
    bench_filter_builder,
    bench_filter_matches_true,
    bench_filter_matches_false,
    bench_filter_matches_permissive,
    bench_filter_clone,
);

criterion_group!(rule_benchmarks, bench_rule_builder, bench_rule_clone,);

criterion_group!(
    config_benchmarks,
    bench_router_config_default,
    bench_router_config_builder,
    bench_router_config_clone,
);

criterion_group!(
    router_benchmarks,
    bench_router_new_empty,
    bench_router_builder,
    bench_router_route_default,
    bench_router_route_single_rule,
    bench_router_route_multiple_rules,
    bench_router_route_drop,
    bench_router_route_multi_topic,
    bench_router_add_rule,
    bench_router_stats,
);

criterion_group!(
    stats_benchmarks,
    bench_routing_stats_default,
    bench_routing_stats_clone,
);

criterion_group!(rule_id_benchmarks, bench_rule_id_new, bench_rule_id_eq,);

criterion_group!(misc_benchmarks, bench_type_sizes, bench_routing_throughput,);

criterion_main!(
    pattern_benchmarks,
    template_benchmarks,
    filter_benchmarks,
    rule_benchmarks,
    config_benchmarks,
    router_benchmarks,
    stats_benchmarks,
    rule_id_benchmarks,
    misc_benchmarks,
);
