# ferric-rules

A (mostly) CLIPS-compatible forward-chaining rules engine, written in Rust.

CLIPS has been around since the 1980s and is battle-tested for expert systems,
but it's a C library with global state, which makes it awkward to embed in
modern applications. ferric-rules keeps the language and semantics — deffacts,
defrule, salience, the Rete algorithm — and drops the parts that don't fit:
no global state, no thread-unsafe singletons, no C build headaches.
Each `Engine` instance is fully independent, and the whole thing compiles
as a normal Rust crate (or as a C library via `ferric-ffi`).

The engine is early but functional: ordered and template facts, negative and
existential patterns, the full Rete join network, modules with focus stacks,
deffunction/defgeneric, globals, and the core CLIPS standard library
(math, string, multifield, predicates, I/O). What's planned, but not here yet:

- logical dependencies
- some of the more exotic pattern connectives
- idiomatic wrappers/bindings (C++, Swift, python, etc.)

We have no plans to support the object system (COOL).

## Example: in-app engagement rules

Here's a real-ish use case. You have a mobile app and want to decide, once per
session, whether to show a rating prompt, an upsell, a paywall, a retention
offer, or nothing at all. The rules encode your product team's priorities and
constraints; your app just asserts what it knows about the user and runs the
engine.

### The rules

```clips
;;; Only one prompt per session. Higher salience = higher priority.
;;; The (prompt-shown) guard stops lower-priority rules from firing
;;; after a decision is made.

;;; Bad session? Show nothing.
(defrule suppress-after-crash
    (declare (salience 100))
    (has-crashed yes)
    =>
    (assert (prompt-suppressed))
    (assert (prompt-shown)))

;;; Free user hit a premium feature — paywall.
(defrule show-paywall
    (declare (salience 90))
    (user-tier free)
    (accessed-premium-feature)
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show paywall))
    (assert (prompt-shown))
    (printout t "ACTION: paywall" crlf))

;;; Brand-new user (<=3 sessions) — signup incentive.
(defrule offer-signup-incentive
    (declare (salience 70))
    (user-tier free)
    (session-count ?s)
    (test (<= ?s 3))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show signup-incentive))
    (assert (prompt-shown))
    (printout t "ACTION: signup-incentive" crlf))

;;; Haven't opened the app in a week — retention discount.
(defrule offer-retention-discount
    (declare (salience 60))
    (days-since-last-open ?d)
    (test (>= ?d 7))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show retention-discount))
    (assert (prompt-shown))
    (printout t "ACTION: retention-discount" crlf))

;;; Engaged user who hasn't rated — ask for a review.
(defrule prompt-app-rating
    (declare (salience 50))
    (session-count ?s)
    (test (>= ?s 10))
    (days-since-install ?d)
    (test (>= ?d 7))
    (has-rated no)
    (not (has-crashed yes))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show rate-app))
    (assert (prompt-shown))
    (printout t "ACTION: rate-app" crlf))

;;; Engaged free user — upsell to paid.
(defrule upsell-to-paid
    (declare (salience 40))
    (user-tier free)
    (session-count ?s)
    (test (>= ?s 5))
    (feature-usage high)
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show upsell-paid))
    (assert (prompt-shown))
    (printout t "ACTION: upsell-paid" crlf))

;;; Paid power user — upsell to premium.
(defrule upsell-to-premium
    (declare (salience 40))
    (user-tier paid)
    (session-count ?s)
    (test (>= ?s 20))
    (feature-usage high)
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show upsell-premium))
    (assert (prompt-shown))
    (printout t "ACTION: upsell-premium" crlf))

;;; User hasn't shared much — offer credits for sharing.
(defrule offer-share-credit
    (declare (salience 30))
    (social-shares ?n)
    (test (< ?n 3))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show share-credit))
    (assert (prompt-shown))
    (printout t "ACTION: share-credit" crlf))
```

### Using it from Rust

Load the rules once, then for each session, assert the current user state and
run the engine. The highest-priority matching rule fires and the `(show ...)`
fact tells you what to do.

```rust
use ferric::runtime::{Engine, RunLimit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rules = include_str!("rules/engagement.clp");

    let mut engine = Engine::with_rules(rules)?;

    // Assert what we know about this user right now.
    engine.assert_ordered_symbol("user-tier", "free")?;
    engine.assert_ordered("session-count", 12_i64)?;
    engine.assert_ordered("days-since-install", 14_i64)?;

    engine.assert_ordered_symbol("has-rated", "no")?;
    engine.assert_ordered_symbol("has-crashed", "no")?;

    engine.assert_ordered_symbol("feature-usage", "high")?;
    engine.assert_ordered("social-shares", 1_i64)?;

    // Run the engine. One rule fires.
    let result = engine.run(RunLimit::Count(100))?;
    assert_eq!(result.rules_fired, 1);

    // Read the decision from the printout channel, or inspect working memory
    // for the (show ...) fact.
    let output = engine.get_output("t").unwrap_or("");
    assert_eq!(output, "ACTION: rate-app\n");

    Ok(())
}
```

### What fires when

Given the rules above, the engine picks one action per session based on the
user's state:

| User state | Action | Why |
|---|---|---|
| Free, 2 sessions | `signup-incentive` | New user, highest eligible priority |
| Free, 12 sessions, hasn't rated | `rate-app` | Engaged + hasn't reviewed yet |
| Free, 12 sessions, has rated, heavy usage | `upsell-paid` | Already rated, using features |
| Paid, 25 sessions, heavy usage | `upsell-premium` | Power user on paid tier |
| Any tier, app just crashed | *(nothing)* | All prompts suppressed |
| Paid, 10 days since last open | `retention-discount` | Lapsed user coming back |
| Free, hit a premium feature | `paywall` | Highest action priority |
| Free, 8 sessions, has rated, low usage, 0 shares | `share-credit` | Nothing else matches |

These scenarios are verified as tests in
[`crates/ferric/tests/clips_compat.rs`](crates/ferric/tests/clips_compat.rs)
(look for `test_engagement_*`), so they won't silently go stale.

## License

MIT OR Apache-2.0
