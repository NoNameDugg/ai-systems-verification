//! Topic Routing for Flash.
//!
//! This module provides intelligent routing of market events to Redis Streams
//! based on configurable rules, patterns, and filters.
//!
//! # Features
//!
//! - **Pattern matching**: Wildcards (`*`, `#`) for flexible topic matching
//! - **Filtering**: Route by exchange, instrument, data type
//! - **Multi-topic routing**: Single event to multiple streams
//! - **Priority-based rules**: Control rule execution order
//! - **Statistics tracking**: Monitor routing behavior
//!
//! # Topic Naming Convention
//!
//! ```text
//! market_data.{exchange}.{base}_{quote}.book    → Order book snapshots
//! market_data.{exchange}.{base}_{quote}.trade   → Trade executions
//! market_data.{exchange}.{base}_{quote}.ticker  → Ticker updates
//! system.flash.health                            → Health heartbeats
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use astra_flash::publisher::topics::{TopicRouter, RoutingRuleBuilder, RoutingFilterBuilder};
//!
//! let router = TopicRouterBuilder::new()
//!     .add_rule(
//!         RoutingRuleBuilder::new("btc-priority")
//!             .filter(RoutingFilterBuilder::new().base_asset("BTC").build())
//!             .target_template("priority.{exchange}.{base}_{quote}.book")
//!             .priority(1)
//!             .build()
//!             .unwrap(),
//!     )
//!     .build();
//!
//! let result = router.route(&event);
//! for topic in result.topics {
//!     println!("Route to: {}", topic);
//! }
//! ```

use crate::core::types::{Exchange, MarketData, MarketEvent};
use crate::publisher::stream::{TopicBuilder, TopicType};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

// =============================================================================
// RULE ID
// =============================================================================

/// Unique identifier for a routing rule.
///
/// Uses atomic counter for thread-safe ID generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(u64);

static RULE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl RuleId {
    /// Create a new unique rule ID.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::RuleId;
    ///
    /// let id1 = RuleId::new();
    /// let id2 = RuleId::new();
    /// assert_ne!(id1, id2);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self(RULE_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the inner value.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.0
    }
}

impl Default for RuleId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<u64> for RuleId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rule-{}", self.0)
    }
}

// =============================================================================
// PATTERN SEGMENT
// =============================================================================

/// Segment type in a topic pattern.
///
/// Used internally by [`TopicPattern`] for matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternSegment {
    /// Exact match segment.
    Exact(String),
    /// Single-segment wildcard (`*`).
    SingleWildcard,
    /// Multi-segment wildcard (`#`).
    MultiWildcard,
}

// =============================================================================
// TOPIC PATTERN
// =============================================================================

/// Pattern for matching topics with wildcards.
///
/// # Wildcards
///
/// - `*` matches exactly one segment
/// - `#` matches zero or more segments
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::TopicPattern;
///
/// let pattern = TopicPattern::parse("market_data.*.btc_usd.book").unwrap();
/// assert!(pattern.matches("market_data.deribit.btc_usd.book"));
/// assert!(pattern.matches("market_data.binance.btc_usd.book"));
/// assert!(!pattern.matches("market_data.deribit.eth_usd.book"));
/// ```
#[derive(Debug, Clone)]
pub struct TopicPattern {
    /// Pattern segments.
    segments: Vec<PatternSegment>,
    /// Original pattern string.
    raw: String,
}

impl TopicPattern {
    /// Parse a pattern string into a `TopicPattern`.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::TopicPattern;
    ///
    /// let pattern = TopicPattern::parse("market_data.#").unwrap();
    /// assert!(pattern.matches("market_data.anything.here"));
    /// ```
    pub fn parse(pattern: &str) -> Result<Self, RoutingError> {
        if pattern.is_empty() {
            return Err(RoutingError::InvalidPattern {
                pattern: pattern.to_string(),
                reason: "Pattern cannot be empty".to_string(),
            });
        }

        let segments: Vec<PatternSegment> = pattern
            .split('.')
            .map(|s| match s {
                "*" => PatternSegment::SingleWildcard,
                "#" => PatternSegment::MultiWildcard,
                _ => PatternSegment::Exact(s.to_lowercase()),
            })
            .collect();

        Ok(Self {
            segments,
            raw: pattern.to_string(),
        })
    }

    /// Check if a topic matches this pattern.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::TopicPattern;
    ///
    /// let pattern = TopicPattern::parse("market_data.deribit.*").unwrap();
    /// assert!(pattern.matches("market_data.deribit.btc_usd"));
    /// assert!(!pattern.matches("market_data.deribit.btc.usd"));
    /// ```
    #[must_use]
    pub fn matches(&self, topic: &str) -> bool {
        let topic_segments: Vec<&str> = topic.split('.').collect();
        self.match_segments(&self.segments, &topic_segments)
    }

    /// Recursive segment matching with wildcard support.
    fn match_segments(&self, pattern: &[PatternSegment], topic: &[&str]) -> bool {
        match (pattern.first(), topic.first()) {
            // Both exhausted - match
            (None, None) => true,

            // Pattern exhausted but topic has more - no match
            (None, Some(_)) => false,

            // Topic exhausted but pattern has more
            (Some(PatternSegment::MultiWildcard), None) => {
                // # can match zero segments
                self.match_segments(&pattern[1..], topic)
            },
            (Some(_), None) => false,

            // Multi-wildcard - try zero or more matches
            (Some(PatternSegment::MultiWildcard), Some(_)) => {
                // Try matching zero segments (skip #)
                self.match_segments(&pattern[1..], topic)
                    // Or try matching one segment (consume one from topic)
                    || self.match_segments(pattern, &topic[1..])
            },

            // Single wildcard - match any single segment
            (Some(PatternSegment::SingleWildcard), Some(_)) => {
                self.match_segments(&pattern[1..], &topic[1..])
            },

            // Exact match
            (Some(PatternSegment::Exact(p)), Some(t)) => {
                p.eq_ignore_ascii_case(t) && self.match_segments(&pattern[1..], &topic[1..])
            },
        }
    }

    /// Get the original pattern string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for TopicPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.raw)
    }
}

// =============================================================================
// TEMPLATE SEGMENT
// =============================================================================

/// Segment type in a topic template.
///
/// Used internally by [`TopicTemplate`] for expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateSegment {
    /// Literal text.
    Literal(String),
    /// Exchange placeholder `{exchange}`.
    Exchange,
    /// Base asset placeholder `{base}`.
    Base,
    /// Quote asset placeholder `{quote}`.
    Quote,
    /// Topic type placeholder `{type}`.
    Type,
    /// Prefix placeholder `{prefix}`.
    Prefix,
}

// =============================================================================
// TOPIC TEMPLATE
// =============================================================================

/// Template for generating topic names with placeholders.
///
/// # Placeholders
///
/// - `{exchange}` - Exchange name (e.g., "deribit")
/// - `{base}` - Base asset (e.g., "btc")
/// - `{quote}` - Quote asset (e.g., "usd")
/// - `{type}` - Topic type (e.g., "book")
/// - `{prefix}` - Topic prefix (e.g., "market_data")
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::TopicTemplate;
/// use astra_flash::core::types::{Exchange, Instrument, MarketData, MarketEvent, MarketEventType};
///
/// let template = TopicTemplate::parse("market_data.{exchange}.{base}_{quote}.book").unwrap();
/// let instrument = Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL");
/// let event = MarketEvent {
///     event_type: MarketEventType::Snapshot,
///     instrument,
///     timestamp: 0,
///     local_timestamp: 0,
///     sequence: None,
///     data: MarketData::Book { bids: vec![], asks: vec![] },
/// };
/// let topic = template.expand(&event);
/// assert_eq!(topic, "market_data.deribit.btc_usd.book");
/// ```
#[derive(Debug, Clone)]
pub struct TopicTemplate {
    /// Parsed segments.
    segments: Vec<TemplateSegment>,
    /// Original template string.
    raw: String,
}

impl TopicTemplate {
    /// Parse a template string.
    ///
    /// # Errors
    ///
    /// Returns error if template syntax is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::TopicTemplate;
    ///
    /// let template = TopicTemplate::parse("{prefix}.{exchange}.book").unwrap();
    /// ```
    pub fn parse(template: &str) -> Result<Self, RoutingError> {
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut in_placeholder = false;

        for c in template.chars() {
            match c {
                '{' => {
                    if in_placeholder {
                        return Err(RoutingError::InvalidTemplate {
                            template: template.to_string(),
                            reason: "Nested placeholders not allowed".to_string(),
                        });
                    }
                    if !current.is_empty() {
                        segments.push(TemplateSegment::Literal(current.clone()));
                        current.clear();
                    }
                    in_placeholder = true;
                },
                '}' => {
                    if !in_placeholder {
                        return Err(RoutingError::InvalidTemplate {
                            template: template.to_string(),
                            reason: "Unexpected closing brace".to_string(),
                        });
                    }
                    let segment = match current.to_lowercase().as_str() {
                        "exchange" => TemplateSegment::Exchange,
                        "base" => TemplateSegment::Base,
                        "quote" => TemplateSegment::Quote,
                        "type" => TemplateSegment::Type,
                        "prefix" => TemplateSegment::Prefix,
                        other => {
                            return Err(RoutingError::InvalidTemplate {
                                template: template.to_string(),
                                reason: format!("Unknown placeholder: {{{other}}}"),
                            })
                        },
                    };
                    segments.push(segment);
                    current.clear();
                    in_placeholder = false;
                },
                _ => {
                    current.push(c);
                },
            }
        }

        if in_placeholder {
            return Err(RoutingError::InvalidTemplate {
                template: template.to_string(),
                reason: "Unclosed placeholder".to_string(),
            });
        }

        if !current.is_empty() {
            segments.push(TemplateSegment::Literal(current));
        }

        Ok(Self {
            segments,
            raw: template.to_string(),
        })
    }

    /// Expand the template using event data.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::TopicTemplate;
    /// use astra_flash::core::types::{Exchange, Instrument, MarketData, MarketEvent, MarketEventType};
    ///
    /// let template = TopicTemplate::parse("market_data.{exchange}.{base}_{quote}.book").unwrap();
    /// let event = MarketEvent {
    ///     event_type: MarketEventType::Snapshot,
    ///     instrument: Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
    ///     timestamp: 0,
    ///     local_timestamp: 0,
    ///     sequence: None,
    ///     data: MarketData::Book { bids: vec![], asks: vec![] },
    /// };
    /// assert_eq!(template.expand(&event), "market_data.deribit.btc_usd.book");
    /// ```
    #[must_use]
    pub fn expand(&self, event: &MarketEvent) -> String {
        let topic_type = Self::detect_topic_type(event);
        self.expand_with_prefix(event, "market_data", topic_type)
    }

    /// Expand the template with explicit prefix and type.
    #[must_use]
    pub fn expand_with_prefix(
        &self,
        event: &MarketEvent,
        prefix: &str,
        topic_type: TopicType,
    ) -> String {
        let mut result = String::with_capacity(64);

        for segment in &self.segments {
            match segment {
                TemplateSegment::Literal(s) => result.push_str(s),
                TemplateSegment::Exchange => {
                    result.push_str(event.instrument.exchange.as_str());
                },
                TemplateSegment::Base => {
                    result.push_str(&event.instrument.base.to_lowercase());
                },
                TemplateSegment::Quote => {
                    result.push_str(&event.instrument.quote.to_lowercase());
                },
                TemplateSegment::Type => {
                    result.push_str(topic_type.as_str());
                },
                TemplateSegment::Prefix => {
                    result.push_str(prefix);
                },
            }
        }

        result
    }

    /// Detect the topic type from an event.
    const fn detect_topic_type(event: &MarketEvent) -> TopicType {
        match &event.data {
            MarketData::Book { .. } => TopicType::Book,
            MarketData::Trade { .. } => TopicType::Trade,
            MarketData::Heartbeat { .. } => TopicType::Event,
        }
    }

    /// Get the original template string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for TopicTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.raw)
    }
}

// =============================================================================
// ROUTING FILTER
// =============================================================================

/// Filter criteria for routing rules.
///
/// All criteria are AND-ed together. Empty lists mean "match all".
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::RoutingFilterBuilder;
/// use astra_flash::publisher::stream::TopicType;
/// use astra_flash::core::types::Exchange;
///
/// let filter = RoutingFilterBuilder::new()
///     .exchange(Exchange::Deribit)
///     .base_asset("BTC")
///     .topic_type(TopicType::Book)
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct RoutingFilter {
    /// Exchanges to match (empty = all).
    pub exchanges: Vec<Exchange>,
    /// Topic types to match (empty = all).
    pub topic_types: Vec<TopicType>,
    /// Base assets to match (empty = all).
    pub base_assets: Vec<String>,
    /// Quote assets to match (empty = all).
    pub quote_assets: Vec<String>,
    /// Enable case-insensitive matching.
    pub case_insensitive: bool,
}

impl RoutingFilter {
    /// Check if an event matches this filter.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::RoutingFilterBuilder;
    /// use astra_flash::core::types::{Exchange, Instrument, MarketData, MarketEvent, MarketEventType};
    ///
    /// let filter = RoutingFilterBuilder::new()
    ///     .exchange(Exchange::Deribit)
    ///     .build();
    ///
    /// let event = MarketEvent {
    ///     event_type: MarketEventType::Snapshot,
    ///     instrument: Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL"),
    ///     timestamp: 0,
    ///     local_timestamp: 0,
    ///     sequence: None,
    ///     data: MarketData::Book { bids: vec![], asks: vec![] },
    /// };
    ///
    /// assert!(filter.matches(&event));
    /// ```
    #[must_use]
    pub fn matches(&self, event: &MarketEvent) -> bool {
        // Exchange filter
        if !self.exchanges.is_empty() && !self.exchanges.contains(&event.instrument.exchange) {
            return false;
        }

        // Topic type filter
        if !self.topic_types.is_empty() {
            let event_type = match &event.data {
                MarketData::Book { .. } => TopicType::Book,
                MarketData::Trade { .. } => TopicType::Trade,
                MarketData::Heartbeat { .. } => TopicType::Event,
            };
            if !self.topic_types.contains(&event_type) {
                return false;
            }
        }

        // Base asset filter
        if !self.base_assets.is_empty() {
            let base = if self.case_insensitive {
                event.instrument.base.to_lowercase()
            } else {
                event.instrument.base.clone()
            };
            let matches = self.base_assets.iter().any(|b| {
                if self.case_insensitive {
                    b.to_lowercase() == base
                } else {
                    b == &base
                }
            });
            if !matches {
                return false;
            }
        }

        // Quote asset filter
        if !self.quote_assets.is_empty() {
            let quote = if self.case_insensitive {
                event.instrument.quote.to_lowercase()
            } else {
                event.instrument.quote.clone()
            };
            let matches = self.quote_assets.iter().any(|q| {
                if self.case_insensitive {
                    q.to_lowercase() == quote
                } else {
                    q == &quote
                }
            });
            if !matches {
                return false;
            }
        }

        true
    }
}

/// Builder for [`RoutingFilter`].
#[derive(Debug, Clone, Default)]
pub struct RoutingFilterBuilder {
    filter: RoutingFilter,
}

impl RoutingFilterBuilder {
    /// Create a new filter builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an exchange to match.
    #[must_use]
    pub fn exchange(mut self, exchange: Exchange) -> Self {
        self.filter.exchanges.push(exchange);
        self
    }

    /// Add multiple exchanges to match.
    #[must_use]
    pub fn exchanges(mut self, exchanges: Vec<Exchange>) -> Self {
        self.filter.exchanges.extend(exchanges);
        self
    }

    /// Add a topic type to match.
    #[must_use]
    pub fn topic_type(mut self, topic_type: TopicType) -> Self {
        self.filter.topic_types.push(topic_type);
        self
    }

    /// Add a base asset to match.
    #[must_use]
    pub fn base_asset(mut self, asset: impl Into<String>) -> Self {
        self.filter.base_assets.push(asset.into());
        self
    }

    /// Add a quote asset to match.
    #[must_use]
    pub fn quote_asset(mut self, asset: impl Into<String>) -> Self {
        self.filter.quote_assets.push(asset.into());
        self
    }

    /// Enable case-insensitive matching.
    #[must_use]
    pub const fn case_insensitive(mut self, enabled: bool) -> Self {
        self.filter.case_insensitive = enabled;
        self
    }

    /// Build the filter.
    #[must_use]
    pub fn build(self) -> RoutingFilter {
        self.filter
    }
}

// =============================================================================
// ROUTING ACTION
// =============================================================================

/// Action to take when a routing rule matches.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::RoutingAction;
///
/// let action = RoutingAction::Route;
/// assert!(matches!(action, RoutingAction::Route));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RoutingAction {
    /// Route to specified topics and continue matching more rules.
    #[default]
    Route,
    /// Route to specified topics and stop matching (no more rules).
    RouteAndStop,
    /// Drop the event (don't publish anywhere).
    Drop,
}

// =============================================================================
// ROUTING RULE
// =============================================================================

/// A single routing rule.
///
/// Rules are evaluated in priority order (lower = higher priority).
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::{RoutingRuleBuilder, RoutingFilterBuilder, RoutingAction};
/// use astra_flash::core::types::Exchange;
///
/// let rule = RoutingRuleBuilder::new("deribit-priority")
///     .priority(1)
///     .filter(RoutingFilterBuilder::new().exchange(Exchange::Deribit).build())
///     .target_template("priority.{exchange}.{base}_{quote}.{type}")
///     .action(RoutingAction::RouteAndStop)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct RoutingRule {
    /// Unique rule identifier.
    pub id: RuleId,
    /// Rule name (for logging/debugging).
    pub name: String,
    /// Topic pattern to match against (optional).
    pub pattern: Option<TopicPattern>,
    /// Filter criteria.
    pub filter: RoutingFilter,
    /// Target topic templates.
    pub targets: Vec<TopicTemplate>,
    /// Rule priority (lower = higher priority).
    pub priority: u32,
    /// Whether this rule is enabled.
    pub enabled: bool,
    /// Action when matched.
    pub action: RoutingAction,
}

impl Default for RoutingRule {
    fn default() -> Self {
        Self {
            id: RuleId::new(),
            name: String::new(),
            pattern: None,
            filter: RoutingFilter::default(),
            targets: Vec::new(),
            priority: 100,
            enabled: true,
            action: RoutingAction::Route,
        }
    }
}

/// Builder for [`RoutingRule`].
#[derive(Debug, Clone)]
pub struct RoutingRuleBuilder {
    rule: RoutingRule,
}

impl RoutingRuleBuilder {
    /// Create a new rule builder with a name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            rule: RoutingRule {
                name: name.into(),
                ..Default::default()
            },
        }
    }

    /// Set the rule priority.
    #[must_use]
    pub const fn priority(mut self, priority: u32) -> Self {
        self.rule.priority = priority;
        self
    }

    /// Set whether the rule is enabled.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.rule.enabled = enabled;
        self
    }

    /// Set the routing filter.
    #[must_use]
    pub fn filter(mut self, filter: RoutingFilter) -> Self {
        self.rule.filter = filter;
        self
    }

    /// Set a topic pattern to match.
    #[must_use]
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        if let Ok(p) = TopicPattern::parse(&pattern.into()) {
            self.rule.pattern = Some(p);
        }
        self
    }

    /// Add a target template.
    #[must_use]
    pub fn target_template(mut self, template: impl Into<String>) -> Self {
        if let Ok(t) = TopicTemplate::parse(&template.into()) {
            self.rule.targets.push(t);
        }
        self
    }

    /// Set the routing action.
    #[must_use]
    pub const fn action(mut self, action: RoutingAction) -> Self {
        self.rule.action = action;
        self
    }

    /// Build the rule.
    ///
    /// # Errors
    ///
    /// Returns error if the rule is invalid.
    pub fn build(self) -> Result<RoutingRule, RoutingError> {
        // Add default target if none specified
        let mut rule = self.rule;
        if rule.targets.is_empty() {
            let default_template =
                TopicTemplate::parse("{prefix}.{exchange}.{base}_{quote}.{type}")?;
            rule.targets.push(default_template);
        }
        Ok(rule)
    }
}

// =============================================================================
// ROUTING RESULT
// =============================================================================

/// Result of routing a single event.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::RoutingResult;
///
/// let result = RoutingResult::default();
/// assert!(result.topics.is_empty());
/// assert!(!result.dropped);
/// ```
#[derive(Debug, Clone, Default)]
pub struct RoutingResult {
    /// Topics to publish to.
    pub topics: Vec<String>,
    /// Rules that matched.
    pub matched_rules: Vec<RuleId>,
    /// Whether event was dropped.
    pub dropped: bool,
    /// Routing latency in microseconds.
    pub latency_us: u64,
}

// =============================================================================
// ROUTING STATS
// =============================================================================

/// Statistics for routing operations.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::RoutingStats;
///
/// let mut stats = RoutingStats::default();
/// stats.events_routed += 1;
/// assert_eq!(stats.events_routed, 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct RoutingStats {
    /// Total events routed.
    pub events_routed: u64,
    /// Events matched by at least one rule.
    pub events_matched: u64,
    /// Events dropped by rules.
    pub events_dropped: u64,
    /// Events with no matching rules.
    pub events_unmatched: u64,
    /// Average routing latency (EMA).
    pub avg_latency_us: f64,
    /// Topics generated count.
    pub topics_generated: u64,
    /// Per-rule match counts.
    pub rule_matches: HashMap<RuleId, u64>,
}

impl RoutingStats {
    /// Update average latency using EMA.
    pub fn update_latency(&mut self, latency_us: f64) {
        const ALPHA: f64 = 0.2;
        self.avg_latency_us = ALPHA.mul_add(latency_us, (1.0 - ALPHA) * self.avg_latency_us);
    }

    /// Reset all statistics.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// =============================================================================
// ROUTER CONFIG
// =============================================================================

/// Configuration for the topic router.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::RouterConfig;
///
/// let config = RouterConfig::default();
/// assert_eq!(config.default_prefix, "market_data");
/// ```
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Enable case-insensitive matching.
    pub case_insensitive: bool,
    /// Default topic prefix.
    pub default_prefix: String,
    /// Enable strict mode (fail on unmatched events).
    pub strict_mode: bool,
    /// Maximum topics per event.
    pub max_topics_per_event: usize,
    /// Enable routing statistics.
    pub track_stats: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            default_prefix: "market_data".to_string(),
            strict_mode: false,
            max_topics_per_event: 10,
            track_stats: true,
        }
    }
}

impl RouterConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid.
    pub fn validate(&self) -> Result<(), RoutingError> {
        if self.default_prefix.is_empty() {
            return Err(RoutingError::ConfigError(
                "default_prefix cannot be empty".to_string(),
            ));
        }

        if self.max_topics_per_event == 0 {
            return Err(RoutingError::ConfigError(
                "max_topics_per_event must be > 0".to_string(),
            ));
        }

        Ok(())
    }
}

/// Builder for [`RouterConfig`].
#[derive(Debug, Clone, Default)]
pub struct RouterConfigBuilder {
    config: RouterConfig,
}

impl RouterConfigBuilder {
    /// Create a new config builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: RouterConfig::default(),
        }
    }

    /// Set case-insensitive matching.
    #[must_use]
    pub const fn case_insensitive(mut self, enabled: bool) -> Self {
        self.config.case_insensitive = enabled;
        self
    }

    /// Set the default topic prefix.
    #[must_use]
    pub fn default_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.default_prefix = prefix.into();
        self
    }

    /// Set strict mode.
    #[must_use]
    pub const fn strict_mode(mut self, enabled: bool) -> Self {
        self.config.strict_mode = enabled;
        self
    }

    /// Set maximum topics per event.
    #[must_use]
    pub const fn max_topics_per_event(mut self, max: usize) -> Self {
        self.config.max_topics_per_event = max;
        self
    }

    /// Set statistics tracking.
    #[must_use]
    pub const fn track_stats(mut self, enabled: bool) -> Self {
        self.config.track_stats = enabled;
        self
    }

    /// Build the configuration.
    #[must_use]
    pub fn build(self) -> RouterConfig {
        self.config
    }
}

// =============================================================================
// ROUTING ERROR
// =============================================================================

/// Errors from routing operations.
#[derive(Debug, Error)]
pub enum RoutingError {
    /// Invalid pattern syntax.
    #[error("Invalid pattern '{pattern}': {reason}")]
    InvalidPattern {
        /// The invalid pattern.
        pattern: String,
        /// Reason for invalidity.
        reason: String,
    },

    /// Invalid template syntax.
    #[error("Invalid template '{template}': {reason}")]
    InvalidTemplate {
        /// The invalid template.
        template: String,
        /// Reason for invalidity.
        reason: String,
    },

    /// Rule not found.
    #[error("Rule not found: {id}")]
    RuleNotFound {
        /// The missing rule ID.
        id: RuleId,
    },

    /// Duplicate rule ID.
    #[error("Duplicate rule ID: {id}")]
    DuplicateRule {
        /// The duplicate rule ID.
        id: RuleId,
    },

    /// Too many topics generated.
    #[error("Too many topics: {count} exceeds max {max}")]
    TooManyTopics {
        /// Number of topics generated.
        count: usize,
        /// Maximum allowed.
        max: usize,
    },

    /// No rules matched in strict mode.
    #[error("No matching rules for event type: {event_type}")]
    NoMatchingRules {
        /// The event type that didn't match.
        event_type: String,
    },

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Result type for routing operations.
pub type RoutingResult2 = Result<RoutingResult, RoutingError>;

// =============================================================================
// TOPIC ROUTER
// =============================================================================

/// Topic router for intelligent message routing.
///
/// Routes market events to Redis Streams based on configurable rules.
///
/// # Thread Safety
///
/// `TopicRouter` is `Send + Sync` and can be safely shared across threads.
///
/// # Example
///
/// ```
/// use astra_flash::publisher::topics::{TopicRouter, TopicRouterBuilder, RoutingRuleBuilder};
///
/// let router = TopicRouterBuilder::new()
///     .add_rule(
///         RoutingRuleBuilder::new("default-rule")
///             .target_template("market_data.{exchange}.{base}_{quote}.{type}")
///             .build()
///             .unwrap(),
///     )
///     .build();
///
/// assert_eq!(router.rule_count(), 1);
/// ```
pub struct TopicRouter {
    /// Routing rules (sorted by priority).
    rules: Vec<RoutingRule>,
    /// Default topic builder.
    topic_builder: TopicBuilder,
    /// Configuration.
    config: RouterConfig,
    /// Statistics.
    stats: Arc<RwLock<RoutingStats>>,
}

impl TopicRouter {
    /// Create a new router with configuration.
    ///
    /// # Example
    ///
    /// ```
    /// use astra_flash::publisher::topics::{TopicRouter, RouterConfig};
    ///
    /// let router = TopicRouter::new(RouterConfig::default());
    /// assert_eq!(router.rule_count(), 0);
    /// ```
    #[must_use]
    pub fn new(config: RouterConfig) -> Self {
        Self {
            rules: Vec::new(),
            topic_builder: TopicBuilder::new(&config.default_prefix),
            config,
            stats: Arc::new(RwLock::new(RoutingStats::default())),
        }
    }

    /// Create a router with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(RouterConfig::default())
    }

    /// Route an event to topics.
    ///
    /// Evaluates rules in priority order and returns matching topics.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = router.route(&event);
    /// for topic in result.topics {
    ///     publisher.publish_raw(&topic, data, format, timestamp).await?;
    /// }
    /// ```
    #[must_use]
    pub fn route(&self, event: &MarketEvent) -> RoutingResult {
        let start = std::time::Instant::now();
        let mut result = RoutingResult::default();
        let mut matched = false;

        // Evaluate rules in priority order
        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }

            // Check filter
            if !rule.filter.matches(event) {
                continue;
            }

            // Rule matches
            matched = true;
            result.matched_rules.push(rule.id);

            // Handle action
            match &rule.action {
                RoutingAction::Route => {
                    // Generate topics from templates
                    for template in &rule.targets {
                        let topic = template.expand_with_prefix(
                            event,
                            &self.config.default_prefix,
                            Self::detect_topic_type(event),
                        );
                        if !result.topics.contains(&topic) {
                            result.topics.push(topic);
                        }
                    }
                },
                RoutingAction::RouteAndStop => {
                    // Generate topics and stop
                    for template in &rule.targets {
                        let topic = template.expand_with_prefix(
                            event,
                            &self.config.default_prefix,
                            Self::detect_topic_type(event),
                        );
                        if !result.topics.contains(&topic) {
                            result.topics.push(topic);
                        }
                    }
                    break;
                },
                RoutingAction::Drop => {
                    result.dropped = true;
                    result.topics.clear();
                    break;
                },
            }

            // Check topic limit
            if result.topics.len() >= self.config.max_topics_per_event {
                result.topics.truncate(self.config.max_topics_per_event);
                break;
            }
        }

        // Apply default routing if no rules matched
        if !matched && !self.config.strict_mode {
            let default_topic = self.default_topic(event);
            result.topics.push(default_topic);
        }

        result.latency_us = start.elapsed().as_micros() as u64;

        // Update statistics
        if self.config.track_stats {
            let mut stats = self.stats.write();
            stats.events_routed += 1;
            if matched {
                stats.events_matched += 1;
            } else {
                stats.events_unmatched += 1;
            }
            if result.dropped {
                stats.events_dropped += 1;
            }
            stats.topics_generated += result.topics.len() as u64;
            stats.update_latency(result.latency_us as f64);

            for id in &result.matched_rules {
                *stats.rule_matches.entry(*id).or_insert(0) += 1;
            }
        }

        result
    }

    /// Get the default topic for an event.
    fn default_topic(&self, event: &MarketEvent) -> String {
        let topic_type = Self::detect_topic_type(event);
        self.topic_builder
            .for_instrument(&event.instrument, topic_type)
    }

    /// Detect the topic type from an event.
    const fn detect_topic_type(event: &MarketEvent) -> TopicType {
        match &event.data {
            MarketData::Book { .. } => TopicType::Book,
            MarketData::Trade { .. } => TopicType::Trade,
            MarketData::Heartbeat { .. } => TopicType::Event,
        }
    }

    /// Add a routing rule.
    ///
    /// Rules are automatically sorted by priority.
    pub fn add_rule(&mut self, rule: RoutingRule) {
        self.rules.push(rule);
        self.rules.sort_by_key(|r| r.priority);
    }

    /// Remove a rule by ID.
    ///
    /// Returns true if the rule was removed.
    pub fn remove_rule(&mut self, id: RuleId) -> bool {
        if let Some(pos) = self.rules.iter().position(|r| r.id == id) {
            self.rules.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get a rule by ID.
    #[must_use]
    pub fn get_rule(&self, id: RuleId) -> Option<&RoutingRule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// Get the number of rules.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Get current statistics.
    #[must_use]
    pub fn stats(&self) -> RoutingStats {
        self.stats.read().clone()
    }

    /// Reset statistics.
    pub fn reset_stats(&self) {
        self.stats.write().reset();
    }

    /// Get the configuration.
    #[must_use]
    pub const fn config(&self) -> &RouterConfig {
        &self.config
    }
}

impl std::fmt::Debug for TopicRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TopicRouter")
            .field("rule_count", &self.rules.len())
            .field("config", &self.config)
            .finish()
    }
}

/// Builder for [`TopicRouter`].
#[derive(Debug, Clone, Default)]
pub struct TopicRouterBuilder {
    config: RouterConfig,
    rules: Vec<RoutingRule>,
}

impl TopicRouterBuilder {
    /// Create a new router builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the router configuration.
    #[must_use]
    pub fn config(mut self, config: RouterConfig) -> Self {
        self.config = config;
        self
    }

    /// Add a routing rule.
    #[must_use]
    pub fn add_rule(mut self, rule: RoutingRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Build the router.
    #[must_use]
    pub fn build(self) -> TopicRouter {
        let mut router = TopicRouter::new(self.config);
        for rule in self.rules {
            router.add_rule(rule);
        }
        router
    }
}

// =============================================================================
// SEND + SYNC VERIFICATION
// =============================================================================

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuleId>();
    assert_send_sync::<TopicPattern>();
    assert_send_sync::<TopicTemplate>();
    assert_send_sync::<RoutingFilter>();
    assert_send_sync::<RoutingAction>();
    assert_send_sync::<RoutingRule>();
    assert_send_sync::<RoutingResult>();
    assert_send_sync::<RoutingStats>();
    assert_send_sync::<RouterConfig>();
    assert_send_sync::<TopicRouter>();
    assert_send_sync::<RoutingError>();
};

// =============================================================================
// INLINE TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{Exchange, Instrument, MarketData, MarketEventType, Side};
    use rust_decimal_macros::dec;

    fn test_instrument() -> Instrument {
        Instrument::new("BTC", "USD", Exchange::Deribit, "BTC-PERPETUAL")
    }

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

    fn test_trade_event() -> MarketEvent {
        MarketEvent {
            event_type: MarketEventType::Delta,
            instrument: test_instrument(),
            timestamp: 1234567890,
            local_timestamp: 1234567890,
            sequence: Some(2),
            data: MarketData::Trade {
                price: 50000.0,
                quantity: dec!(1.0),
                side: Side::Bid,
                trade_id: Some("trade123".to_string()),
            },
        }
    }

    #[test]
    fn test_rule_id_unique() {
        let id1 = RuleId::new();
        let id2 = RuleId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_pattern_exact() {
        let pattern = TopicPattern::parse("market_data.deribit.btc_usd.book").unwrap();
        assert!(pattern.matches("market_data.deribit.btc_usd.book"));
        assert!(!pattern.matches("market_data.binance.btc_usd.book"));
    }

    #[test]
    fn test_pattern_wildcard() {
        let pattern = TopicPattern::parse("market_data.*.btc_usd.book").unwrap();
        assert!(pattern.matches("market_data.deribit.btc_usd.book"));
        assert!(pattern.matches("market_data.binance.btc_usd.book"));
    }

    #[test]
    fn test_pattern_multi_wildcard() {
        let pattern = TopicPattern::parse("market_data.#").unwrap();
        assert!(pattern.matches("market_data.deribit.btc_usd.book"));
        assert!(pattern.matches("market_data"));
    }

    #[test]
    fn test_template_expand() {
        let template = TopicTemplate::parse("market_data.{exchange}.{base}_{quote}.book").unwrap();
        let event = test_book_event();
        let result = template.expand(&event);
        assert_eq!(result, "market_data.deribit.btc_usd.book");
    }

    #[test]
    fn test_filter_empty_matches_all() {
        let filter = RoutingFilter::default();
        assert!(filter.matches(&test_book_event()));
        assert!(filter.matches(&test_trade_event()));
    }

    #[test]
    fn test_filter_exchange() {
        let filter = RoutingFilterBuilder::new()
            .exchange(Exchange::Deribit)
            .build();
        assert!(filter.matches(&test_book_event()));
    }

    #[test]
    fn test_router_route() {
        let router = TopicRouterBuilder::new()
            .add_rule(
                RoutingRuleBuilder::new("test")
                    .target_template("market_data.{exchange}.{base}_{quote}.{type}")
                    .build()
                    .unwrap(),
            )
            .build();

        let result = router.route(&test_book_event());
        assert!(!result.topics.is_empty());
        assert!(result.topics[0].contains("deribit"));
    }

    #[test]
    fn test_router_default_topic() {
        let router = TopicRouter::new(RouterConfig::default());
        let result = router.route(&test_book_event());
        assert!(!result.topics.is_empty());
    }

    #[test]
    fn test_router_drop_action() {
        let router = TopicRouterBuilder::new()
            .add_rule(
                RoutingRuleBuilder::new("drop")
                    .action(RoutingAction::Drop)
                    .build()
                    .unwrap(),
            )
            .build();

        let result = router.route(&test_book_event());
        assert!(result.dropped);
        assert!(result.topics.is_empty());
    }
}
