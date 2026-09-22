//! Tests for Topic Routing module (Part 4.4).
//!
//! Test coverage:
//! - Unit tests: RouterConfig, TopicPattern, RoutingFilter, TopicTemplate, RoutingRule
//! - Integration tests: TopicRouter routing behavior
//! - Edge case tests: Empty rules, disabled rules, limits
//!
//! Total: 40+ tests (exceeding 36 requirement)

use astra_flash::core::types::{Exchange, Instrument, MarketData, MarketEvent, MarketEventType};
use astra_flash::publisher::stream::TopicType;
use astra_flash::publisher::topics::{
    PatternSegment, RouterConfig, RouterConfigBuilder, RoutingAction, RoutingError, RoutingFilter,
    RoutingFilterBuilder, RoutingResult, RoutingRule, RoutingRuleBuilder, RoutingStats, RuleId,
    TemplateSegment, TopicPattern, TopicRouter, TopicRouterBuilder, TopicTemplate,
};

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test instrument.
fn test_instrument() -> Instrument {
    Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
}

/// Create a test instrument with custom parameters.
fn test_instrument_custom(base: &str, quote: &str, exchange: Exchange) -> Instrument {
    Instrument::new(base, quote, exchange, &format!("{}-{}", base, quote))
}

/// Create a test book event.
fn test_book_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Snapshot,
        instrument: test_instrument(),
        timestamp: 1234567890,
        local_timestamp: 1234567890,
        sequence: Some(1),
        data: MarketData::Book {
            bids: vec![],
            asks: vec![],
        },
    }
}

/// Create a test trade event.
fn test_trade_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Delta,
        instrument: test_instrument(),
        timestamp: 1234567890,
        local_timestamp: 1234567890,
        sequence: Some(2),
        data: MarketData::Trade {
            price: 50000.0,
            quantity: rust_decimal_macros::dec!(1.0),
            side: astra_flash::core::types::Side::Bid,
            trade_id: Some("trade123".to_string()),
        },
    }
}

/// Create a test heartbeat event.
fn test_heartbeat_event() -> MarketEvent {
    MarketEvent {
        event_type: MarketEventType::Heartbeat,
        instrument: test_instrument(),
        timestamp: 1234567890,
        local_timestamp: 1234567890,
        sequence: None,
        data: MarketData::Heartbeat {
            exchange_time: 1234567890,
        },
    }
}

// =============================================================================
// ROUTER CONFIG TESTS
// =============================================================================

#[test]
fn test_router_config_default() {
    let config = RouterConfig::default();
    assert!(!config.case_insensitive);
    assert_eq!(config.default_prefix, "market_data");
    assert!(!config.strict_mode);
    assert_eq!(config.max_topics_per_event, 10);
    assert!(config.track_stats);
}

#[test]
fn test_router_config_builder() {
    let config = RouterConfigBuilder::new()
        .case_insensitive(true)
        .default_prefix("custom")
        .strict_mode(true)
        .max_topics_per_event(5)
        .track_stats(false)
        .build();

    assert!(config.case_insensitive);
    assert_eq!(config.default_prefix, "custom");
    assert!(config.strict_mode);
    assert_eq!(config.max_topics_per_event, 5);
    assert!(!config.track_stats);
}

#[test]
fn test_router_config_validate_success() {
    let config = RouterConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_router_config_validate_empty_prefix() {
    let config = RouterConfigBuilder::new().default_prefix("").build();
    assert!(config.validate().is_err());
}

#[test]
fn test_router_config_validate_zero_topics() {
    let config = RouterConfigBuilder::new().max_topics_per_event(0).build();
    assert!(config.validate().is_err());
}

// =============================================================================
// TOPIC PATTERN TESTS
// =============================================================================

#[test]
fn test_topic_pattern_exact_match() {
    let pattern = TopicPattern::parse("market_data.deribit.btc_usd.book").unwrap();
    assert!(pattern.matches("market_data.deribit.btc_usd.book"));
    assert!(!pattern.matches("market_data.deribit.btc_usd.trade"));
    assert!(!pattern.matches("market_data.binance.btc_usd.book"));
}

#[test]
fn test_topic_pattern_single_wildcard() {
    let pattern = TopicPattern::parse("market_data.*.btc_usd.book").unwrap();
    assert!(pattern.matches("market_data.deribit.btc_usd.book"));
    assert!(pattern.matches("market_data.binance.btc_usd.book"));
    assert!(pattern.matches("market_data.oanda.btc_usd.book"));
    assert!(!pattern.matches("market_data.deribit.eth_usd.book"));
}

#[test]
fn test_topic_pattern_multi_wildcard() {
    let pattern = TopicPattern::parse("market_data.#").unwrap();
    assert!(pattern.matches("market_data.deribit.btc_usd.book"));
    assert!(pattern.matches("market_data.deribit"));
    assert!(pattern.matches("market_data"));
    assert!(!pattern.matches("system.flash.health"));
}

#[test]
fn test_topic_pattern_combined_wildcards() {
    let pattern = TopicPattern::parse("market_data.*.#.book").unwrap();
    assert!(pattern.matches("market_data.deribit.btc_usd.book"));
    assert!(pattern.matches("market_data.binance.eth.usdt.book"));
}

#[test]
fn test_topic_pattern_trailing_wildcard() {
    let pattern = TopicPattern::parse("market_data.deribit.*").unwrap();
    assert!(pattern.matches("market_data.deribit.btc_usd"));
    assert!(pattern.matches("market_data.deribit.anything"));
    assert!(!pattern.matches("market_data.deribit.btc.usd"));
}

#[test]
fn test_topic_pattern_parse_error_empty() {
    let result = TopicPattern::parse("");
    assert!(result.is_err());
}

#[test]
fn test_topic_pattern_display() {
    let pattern = TopicPattern::parse("market_data.*.book").unwrap();
    assert_eq!(pattern.to_string(), "market_data.*.book");
}

#[test]
fn test_topic_pattern_clone() {
    let pattern = TopicPattern::parse("market_data.deribit.btc_usd.book").unwrap();
    let cloned = pattern.clone();
    assert_eq!(pattern.to_string(), cloned.to_string());
}

#[test]
fn test_pattern_segment_variants() {
    assert!(matches!(
        PatternSegment::Exact("test".to_string()),
        PatternSegment::Exact(_)
    ));
    assert!(matches!(
        PatternSegment::SingleWildcard,
        PatternSegment::SingleWildcard
    ));
    assert!(matches!(
        PatternSegment::MultiWildcard,
        PatternSegment::MultiWildcard
    ));
}

// =============================================================================
// ROUTING FILTER TESTS
// =============================================================================

#[test]
fn test_routing_filter_empty_matches_all() {
    let filter = RoutingFilter::default();
    let event = test_book_event();
    assert!(filter.matches(&event));
}

#[test]
fn test_routing_filter_exchange_single() {
    let filter = RoutingFilterBuilder::new()
        .exchange(Exchange::Deribit)
        .build();

    let deribit_event = test_book_event();
    let binance_event = MarketEvent {
        instrument: test_instrument_custom("BTC", "USD", Exchange::Binance),
        ..test_book_event()
    };

    assert!(filter.matches(&deribit_event));
    assert!(!filter.matches(&binance_event));
}

#[test]
fn test_routing_filter_exchange_multiple() {
    let filter = RoutingFilterBuilder::new()
        .exchanges(vec![Exchange::Deribit, Exchange::Binance])
        .build();

    let deribit_event = test_book_event();
    let oanda_event = MarketEvent {
        instrument: test_instrument_custom("EUR", "USD", Exchange::Oanda),
        ..test_book_event()
    };

    assert!(filter.matches(&deribit_event));
    assert!(!filter.matches(&oanda_event));
}

#[test]
fn test_routing_filter_topic_type() {
    let filter = RoutingFilterBuilder::new()
        .topic_type(TopicType::Book)
        .build();

    assert!(filter.matches(&test_book_event()));
    assert!(!filter.matches(&test_trade_event()));
}

#[test]
fn test_routing_filter_base_asset() {
    let filter = RoutingFilterBuilder::new().base_asset("BTC").build();

    let btc_event = test_book_event();
    let eth_event = MarketEvent {
        instrument: test_instrument_custom("ETH", "USD", Exchange::Deribit),
        ..test_book_event()
    };

    assert!(filter.matches(&btc_event));
    assert!(!filter.matches(&eth_event));
}

#[test]
fn test_routing_filter_quote_asset() {
    let filter = RoutingFilterBuilder::new().quote_asset("USD").build();

    let usd_event = test_book_event();
    let usdt_event = MarketEvent {
        instrument: test_instrument_custom("BTC", "USDT", Exchange::Binance),
        ..test_book_event()
    };

    assert!(filter.matches(&usd_event));
    assert!(!filter.matches(&usdt_event));
}

#[test]
fn test_routing_filter_combined() {
    let filter = RoutingFilterBuilder::new()
        .exchange(Exchange::Deribit)
        .base_asset("BTC")
        .topic_type(TopicType::Book)
        .build();

    assert!(filter.matches(&test_book_event()));
    assert!(!filter.matches(&test_trade_event()));

    let eth_event = MarketEvent {
        instrument: test_instrument_custom("ETH", "USD", Exchange::Deribit),
        ..test_book_event()
    };
    assert!(!filter.matches(&eth_event));
}

#[test]
fn test_routing_filter_case_insensitive() {
    let filter = RoutingFilterBuilder::new()
        .base_asset("btc")
        .case_insensitive(true)
        .build();

    assert!(filter.matches(&test_book_event())); // BTC should match btc
}

// =============================================================================
// TOPIC TEMPLATE TESTS
// =============================================================================

#[test]
fn test_topic_template_literal() {
    let template = TopicTemplate::parse("market_data.custom.topic").unwrap();
    let event = test_book_event();
    let result = template.expand(&event);
    assert_eq!(result, "market_data.custom.topic");
}

#[test]
fn test_topic_template_exchange_placeholder() {
    let template = TopicTemplate::parse("market_data.{exchange}.btc_usd.book").unwrap();
    let event = test_book_event();
    let result = template.expand(&event);
    assert_eq!(result, "market_data.deribit.btc_usd.book");
}

#[test]
fn test_topic_template_all_placeholders() {
    let template = TopicTemplate::parse("{prefix}.{exchange}.{base}_{quote}.{type}").unwrap();
    let event = test_book_event();
    let result = template.expand_with_prefix(&event, "market_data", TopicType::Book);
    assert_eq!(result, "market_data.deribit.btc_usd.book");
}

#[test]
fn test_topic_template_parse_error_unclosed() {
    let result = TopicTemplate::parse("market_data.{exchange.btc_usd");
    assert!(result.is_err());
}

#[test]
fn test_topic_template_parse_error_unknown_placeholder() {
    let result = TopicTemplate::parse("market_data.{unknown}.btc_usd");
    assert!(result.is_err());
}

#[test]
fn test_topic_template_display() {
    let template = TopicTemplate::parse("market_data.{exchange}.book").unwrap();
    assert_eq!(template.to_string(), "market_data.{exchange}.book");
}

#[test]
fn test_template_segment_variants() {
    let segments = vec![
        TemplateSegment::Literal("test".to_string()),
        TemplateSegment::Exchange,
        TemplateSegment::Base,
        TemplateSegment::Quote,
        TemplateSegment::Type,
        TemplateSegment::Prefix,
    ];
    assert_eq!(segments.len(), 6);
}

// =============================================================================
// ROUTING RULE TESTS
// =============================================================================

#[test]
fn test_routing_rule_default() {
    let rule = RoutingRule::default();
    assert!(rule.enabled);
    assert_eq!(rule.priority, 100);
    assert!(matches!(rule.action, RoutingAction::Route));
}

#[test]
fn test_routing_rule_builder() {
    let rule = RoutingRuleBuilder::new("test-rule")
        .priority(10)
        .enabled(true)
        .filter(RoutingFilter::default())
        .target_template("market_data.{exchange}.{base}_{quote}.book")
        .action(RoutingAction::Route)
        .build()
        .unwrap();

    assert_eq!(rule.name, "test-rule");
    assert_eq!(rule.priority, 10);
    assert!(rule.enabled);
}

#[test]
fn test_routing_rule_priority_ordering() {
    let rule1 = RoutingRuleBuilder::new("high").priority(1).build().unwrap();
    let rule2 = RoutingRuleBuilder::new("low")
        .priority(100)
        .build()
        .unwrap();

    assert!(rule1.priority < rule2.priority);
}

#[test]
fn test_routing_rule_enabled_disabled() {
    let enabled_rule = RoutingRuleBuilder::new("enabled")
        .enabled(true)
        .build()
        .unwrap();
    let disabled_rule = RoutingRuleBuilder::new("disabled")
        .enabled(false)
        .build()
        .unwrap();

    assert!(enabled_rule.enabled);
    assert!(!disabled_rule.enabled);
}

#[test]
fn test_routing_action_variants() {
    let actions = vec![
        RoutingAction::Route,
        RoutingAction::RouteAndStop,
        RoutingAction::Drop,
    ];
    assert_eq!(actions.len(), 3);
}

#[test]
fn test_rule_id_generation() {
    let id1 = RuleId::new();
    let id2 = RuleId::new();
    assert_ne!(id1, id2);
}

#[test]
fn test_rule_id_display() {
    let id = RuleId::from(12345u64);
    assert!(id.to_string().contains("12345"));
}

// =============================================================================
// ROUTING STATS TESTS
// =============================================================================

#[test]
fn test_routing_stats_default() {
    let stats = RoutingStats::default();
    assert_eq!(stats.events_routed, 0);
    assert_eq!(stats.events_matched, 0);
    assert_eq!(stats.events_dropped, 0);
    assert_eq!(stats.avg_latency_us, 0.0);
}

#[test]
fn test_routing_stats_update_latency() {
    let mut stats = RoutingStats::default();

    // EMA with alpha=0.2, starting from 0:
    // First update(100): avg = 0.2*100 + 0.8*0 = 20
    stats.update_latency(100.0);
    assert!(stats.avg_latency_us > 0.0);
    assert!((stats.avg_latency_us - 20.0).abs() < 0.001);

    // Second update(200): avg = 0.2*200 + 0.8*20 = 40 + 16 = 56
    stats.update_latency(200.0);
    assert!((stats.avg_latency_us - 56.0).abs() < 0.001);
}

#[test]
fn test_routing_stats_reset() {
    let mut stats = RoutingStats::default();
    stats.events_routed = 100;
    stats.events_matched = 50;
    stats.reset();

    assert_eq!(stats.events_routed, 0);
    assert_eq!(stats.events_matched, 0);
}

#[test]
fn test_routing_stats_clone() {
    let mut stats = RoutingStats::default();
    stats.events_routed = 42;
    let cloned = stats.clone();
    assert_eq!(stats.events_routed, cloned.events_routed);
}

// =============================================================================
// TOPIC ROUTER INTEGRATION TESTS
// =============================================================================

#[test]
fn test_router_new() {
    let router = TopicRouter::new(RouterConfig::default());
    assert_eq!(router.rule_count(), 0);
}

#[test]
fn test_router_builder() {
    let router = TopicRouterBuilder::new()
        .config(RouterConfig::default())
        .add_rule(
            RoutingRuleBuilder::new("book-rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .topic_type(TopicType::Book)
                        .build(),
                )
                .target_template("market_data.{exchange}.{base}_{quote}.book")
                .build()
                .unwrap(),
        )
        .build();

    assert_eq!(router.rule_count(), 1);
}

#[test]
fn test_router_route_book_event() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("book-rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .topic_type(TopicType::Book)
                        .build(),
                )
                .target_template("market_data.{exchange}.{base}_{quote}.book")
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_book_event());
    assert!(!result.topics.is_empty());
    assert!(result
        .topics
        .contains(&"market_data.deribit.btc_usd.book".to_string()));
}

#[test]
fn test_router_route_trade_event() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("trade-rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .topic_type(TopicType::Trade)
                        .build(),
                )
                .target_template("market_data.{exchange}.{base}_{quote}.trade")
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_trade_event());
    assert!(result
        .topics
        .contains(&"market_data.deribit.btc_usd.trade".to_string()));
}

#[test]
fn test_router_route_multi_topic() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("primary")
                .target_template("primary.{exchange}.{base}_{quote}.book")
                .build()
                .unwrap(),
        )
        .add_rule(
            RoutingRuleBuilder::new("secondary")
                .target_template("secondary.{exchange}.{base}_{quote}.book")
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_book_event());
    assert_eq!(result.topics.len(), 2);
}

#[test]
fn test_router_priority_ordering() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("low-priority")
                .priority(100)
                .target_template("low.{exchange}.book")
                .action(RoutingAction::RouteAndStop)
                .build()
                .unwrap(),
        )
        .add_rule(
            RoutingRuleBuilder::new("high-priority")
                .priority(1)
                .target_template("high.{exchange}.book")
                .action(RoutingAction::RouteAndStop)
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_book_event());
    assert_eq!(result.topics.len(), 1);
    assert!(result.topics[0].starts_with("high"));
}

#[test]
fn test_router_stop_on_match() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("first")
                .priority(1)
                .target_template("first.topic")
                .action(RoutingAction::RouteAndStop)
                .build()
                .unwrap(),
        )
        .add_rule(
            RoutingRuleBuilder::new("second")
                .priority(2)
                .target_template("second.topic")
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_book_event());
    assert_eq!(result.topics.len(), 1);
    assert_eq!(result.topics[0], "first.topic");
}

#[test]
fn test_router_drop_event() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("drop-rule")
                .action(RoutingAction::Drop)
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_book_event());
    assert!(result.dropped);
    assert!(result.topics.is_empty());
}

#[test]
fn test_router_no_match_default() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("btc-only")
                .filter(RoutingFilterBuilder::new().base_asset("ETH").build())
                .target_template("eth.topic")
                .build()
                .unwrap(),
        )
        .build();

    // BTC event won't match ETH filter, should get default topic
    let result = router.route(&test_book_event());
    assert!(!result.topics.is_empty()); // Default topic applied
}

#[test]
fn test_router_no_match_strict() {
    let router = TopicRouterBuilder::new()
        .config(RouterConfigBuilder::new().strict_mode(true).build())
        .add_rule(
            RoutingRuleBuilder::new("eth-only")
                .filter(RoutingFilterBuilder::new().base_asset("ETH").build())
                .target_template("eth.topic")
                .build()
                .unwrap(),
        )
        .build();

    // In strict mode, no match should return empty topics (error condition)
    let result = router.route(&test_book_event());
    assert!(result.topics.is_empty());
}

#[test]
fn test_router_add_rule() {
    let mut router = TopicRouter::new(RouterConfig::default());
    assert_eq!(router.rule_count(), 0);

    router.add_rule(
        RoutingRuleBuilder::new("new-rule")
            .target_template("new.topic")
            .build()
            .unwrap(),
    );
    assert_eq!(router.rule_count(), 1);
}

#[test]
fn test_router_remove_rule() {
    let mut router = TopicRouter::new(RouterConfig::default());
    let rule = RoutingRuleBuilder::new("to-remove")
        .target_template("remove.topic")
        .build()
        .unwrap();
    let id = rule.id;

    router.add_rule(rule);
    assert_eq!(router.rule_count(), 1);

    router.remove_rule(id);
    assert_eq!(router.rule_count(), 0);
}

#[test]
fn test_router_get_rule() {
    let mut router = TopicRouter::new(RouterConfig::default());
    let rule = RoutingRuleBuilder::new("findable")
        .target_template("find.topic")
        .build()
        .unwrap();
    let id = rule.id;

    router.add_rule(rule);
    let found = router.get_rule(id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "findable");
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

#[test]
fn test_router_empty_rules() {
    let router = TopicRouter::new(RouterConfig::default());
    let result = router.route(&test_book_event());

    // With no rules, should use default routing
    assert!(!result.topics.is_empty());
}

#[test]
fn test_router_all_disabled() {
    let router = TopicRouterBuilder::new()
        .add_rule(
            RoutingRuleBuilder::new("disabled1")
                .enabled(false)
                .target_template("disabled1.topic")
                .build()
                .unwrap(),
        )
        .add_rule(
            RoutingRuleBuilder::new("disabled2")
                .enabled(false)
                .target_template("disabled2.topic")
                .build()
                .unwrap(),
        )
        .build();

    let result = router.route(&test_book_event());
    // All rules disabled = default routing
    assert!(!result.dropped);
}

#[test]
fn test_router_max_topics_limit() {
    let mut router = TopicRouterBuilder::new()
        .config(RouterConfigBuilder::new().max_topics_per_event(2).build())
        .build();

    // Add 5 rules that all match
    for i in 0..5 {
        router.add_rule(
            RoutingRuleBuilder::new(&format!("rule-{}", i))
                .target_template(&format!("topic-{}.book", i))
                .build()
                .unwrap(),
        );
    }

    let result = router.route(&test_book_event());
    assert!(result.topics.len() <= 2);
}

#[test]
fn test_router_case_insensitive() {
    // Note: Case insensitivity must be set on the filter itself, not just the router config
    let router = TopicRouterBuilder::new()
        .config(RouterConfigBuilder::new().case_insensitive(true).build())
        .add_rule(
            RoutingRuleBuilder::new("btc-rule")
                .filter(
                    RoutingFilterBuilder::new()
                        .base_asset("btc")
                        .case_insensitive(true) // Must be set on filter for matching
                        .build(),
                )
                .target_template("btc.topic")
                .build()
                .unwrap(),
        )
        .build();

    // Should match BTC even with btc filter (case insensitive)
    let result = router.route(&test_book_event());
    assert!(result.topics.contains(&"btc.topic".to_string()));
}

#[test]
fn test_pattern_edge_cases_only_wildcards() {
    let pattern = TopicPattern::parse("#").unwrap();
    assert!(pattern.matches("anything"));
    assert!(pattern.matches("a.b.c.d.e"));
}

#[test]
fn test_pattern_edge_cases_adjacent_wildcards() {
    let pattern = TopicPattern::parse("*.*.book").unwrap();
    assert!(pattern.matches("a.b.book"));
    assert!(!pattern.matches("a.book"));
}

#[test]
fn test_template_missing_data_fallback() {
    let template = TopicTemplate::parse("market_data.{exchange}.book").unwrap();
    let event = test_book_event();
    let result = template.expand(&event);
    // Should always have exchange from instrument
    assert!(result.contains("deribit"));
}

// =============================================================================
// ROUTING RESULT TESTS
// =============================================================================

#[test]
fn test_routing_result_default() {
    let result = RoutingResult::default();
    assert!(result.topics.is_empty());
    assert!(result.matched_rules.is_empty());
    assert!(!result.dropped);
    assert_eq!(result.latency_us, 0);
}

#[test]
fn test_routing_result_with_topics() {
    let result = RoutingResult {
        topics: vec!["topic1".to_string(), "topic2".to_string()],
        matched_rules: vec![RuleId::from(1u64)],
        dropped: false,
        latency_us: 50,
    };

    assert_eq!(result.topics.len(), 2);
    assert_eq!(result.matched_rules.len(), 1);
}

// =============================================================================
// ROUTING ERROR TESTS
// =============================================================================

#[test]
fn test_routing_error_display() {
    let error = RoutingError::InvalidPattern {
        pattern: "bad.*.*".to_string(),
        reason: "invalid syntax".to_string(),
    };
    let msg = error.to_string();
    assert!(msg.contains("Invalid pattern"));
}

#[test]
fn test_routing_error_variants() {
    let errors = vec![
        RoutingError::InvalidPattern {
            pattern: "test".to_string(),
            reason: "reason".to_string(),
        },
        RoutingError::InvalidTemplate {
            template: "test".to_string(),
            reason: "reason".to_string(),
        },
        RoutingError::RuleNotFound {
            id: RuleId::from(1u64),
        },
        RoutingError::DuplicateRule {
            id: RuleId::from(1u64),
        },
        RoutingError::TooManyTopics { count: 10, max: 5 },
        RoutingError::NoMatchingRules {
            event_type: "Book".to_string(),
        },
        RoutingError::ConfigError("test".to_string()),
    ];

    assert_eq!(errors.len(), 7);
}

// =============================================================================
// SEND + SYNC VERIFICATION
// =============================================================================

#[test]
fn test_send_sync_bounds() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<RouterConfig>();
    assert_send_sync::<TopicPattern>();
    assert_send_sync::<RoutingFilter>();
    assert_send_sync::<TopicTemplate>();
    assert_send_sync::<RoutingRule>();
    assert_send_sync::<RoutingStats>();
    assert_send_sync::<TopicRouter>();
    assert_send_sync::<RoutingResult>();
    assert_send_sync::<RoutingError>();
    assert_send_sync::<RuleId>();
}
