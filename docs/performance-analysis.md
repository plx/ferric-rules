# Phase 6 Performance Analysis

## Benchmark Results (Phase 6 Baseline)

### Engine Microbenchmarks

| Benchmark | Time | Description |
|-----------|------|-------------|
| `engine_create` | ~380 ns | Bare engine construction |
| `load_and_run_simple` | ~9.0 us | Full pipeline: 3 facts, 1 rule |
| `load_and_run_chain_4` | ~18.3 us | Full pipeline: 4-step rule chain |
| `reset_run_simple` | ~2.6 us | Reset + run: 3 facts, 1 rule |
| `reset_run_20_facts` | ~17.2 us | Reset + run: 20 facts, 1 rule |
| `reset_run_negation` | ~3.1 us | Reset + run with negation |
| `reset_run_join_3` | ~4.5 us | Reset + run: 2-pattern join, 3 pairs |
| `reset_run_retract_3` | ~4.1 us | Reset + run: retract cycle, 3 facts |
| `compile_template_rule` | ~7.4 us | Parse + load template + rule |

### Waltz (Line Labeling)

| Scale | Time |
|-------|------|
| 5 junctions | ~65 us |
| 20 junctions | ~175 us |
| 50 junctions | ~424 us |
| 100 junctions | ~917 us |

### Manners (Seating)

| Scale | Time |
|-------|------|
| 8 guests | ~58 us |
| 16 guests | ~83 us |
| 32 guests | ~133 us |
| 64 guests | ~231 us |

## Comparison to Section 14 Targets

| Metric | Target | Measured | Headroom |
|--------|--------|----------|----------|
| Rules fired/sec | >=10,000 | ~1,170,000 | 117x |
| Fact assertions/sec | >=50,000 | ~2,400,000 | 48x |
| Fact retractions/sec | >=20,000 | ~720,000 | 36x |
| Waltz benchmark | <=10s | 917 us | >10,000x |
| Manners 64 | <=5s | 231 us | >20,000x |

All targets are exceeded by at least 36x. Performance is well within budget.

Note: Waltz and Manners benchmarks use simplified workloads that exercise the
same Rete hot paths (template matching, modify, negation, join, retraction)
as the classic benchmarks but at smaller problem sizes.

## Scaling Analysis

Both workloads scale approximately linearly with input size:

- Waltz: 5->100 junctions = 65->917 us (14x for 20x input), O(n log n)
- Manners: 8->64 guests = 58->231 us (4x for 8x input), O(n)

## Optimization Applied: Alpha Memory Reverse Index

Added a `FactId -> Vec<AlphaMemoryId>` reverse index to `AlphaNetwork`:
- Populated during `assert_fact` (O(1) HashMap insert per accepted memory)
- Pruned during `retract_fact` (O(1) HashMap remove)
- Used in `memories_containing_fact` (O(1) lookup replaces O(M) scan)

### Measured Impact

| Benchmark | Change | Interpretation |
|-----------|--------|----------------|
| `reset_run_retract_3` | -2.2% | Retraction path improved |
| `reset_run_join_3` | -1.8% | Join path improved |
| `manners_8_run_only` | -1.5% | Retraction-heavy scenario improved |
| `waltz_100_junctions` | -0.4% | Slight improvement at scale |
| `reset_run_20_facts` | +3.9% | Assertion-heavy overhead |
| `manners_64` | +1.4% | Mixed workload, slight overhead |
| Others | <1% | Within noise threshold |

The reverse index trades small assertion overhead for improved retraction
performance. The trade-off becomes increasingly favorable as the number of
alpha memories grows (eliminating O(M) scans).

## Remaining Optimization Opportunities

The following optimizations were identified during profiling but deferred
because current performance already exceeds all targets by 36-117x:

### High Impact (deferred)
1. **Rc<[T]> for beta node data**: Replace `Vec<JoinTest>`, `Vec<(SlotIndex, VarId)>`,
   `Vec<NodeId>` with `Rc<[T]>` in BetaNode variants. Cloning becomes refcount
   bump instead of heap allocation on every join activation.
2. **Token fact list elimination**: Tokens store full `SmallVec<[FactId; 4]>`
   but also store parent pointer. Reconstruct fact list by walking parent chain
   only when needed (at RHS firing time).
3. **Rc<Value> allocation in bindings**: Every variable binding allocates
   `Rc::new(value.clone())`. For Copy-width values (Integer, Symbol, Float),
   an inline representation would eliminate the heap allocation.

### Medium Impact (deferred)
4. **Focus-aware agenda dispatch**: `pop_matching` linearly scans BTreeMap.
   Per-module activation count or index would make focus dispatch O(log N).
5. **Negative memory cleanup reverse index**: Full scan of all negative/NCC/exists
   memories on token removal. Reverse index would make this O(1).
6. **Fact clone elimination**: `propagate_fact_assertion` and `retract` both
   clone the Fact. Restructuring the API to pass FactId and look up internally
   would eliminate these copies.

### Low Impact (deferred)
7. **AgendaKey SmallVec clone**: For Lex/Mea strategies only.
8. **Deffact clone in reset()**: Arc<[Value]> for copy-on-write.
9. **Thread affinity check caching**: Cache for duration of `run()`.

These opportunities are documented here for future phases if tighter performance
targets are needed.
