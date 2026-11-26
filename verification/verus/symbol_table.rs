//! Verus model of `SymbolTable` interning, checkpoint and restore
//! (crates/ferric-rules-core/src/symbol.rs).
//!
//! # What is proved (unbounded: any table size, any string length, any
//! number of operations)
//!
//! * `lemma_new_wf`: the empty table is well formed.
//! * `lemma_pool_intern`: interning into a well-formed pool keeps it well
//!   formed, only appends, and returns the id at which the string is stored;
//!   an already-present string returns its existing id and changes nothing.
//! * `theorem_intern_symbol`: exact characterization of `intern_symbol`:
//!   it fails iff the mode keeps symbols ASCII and the input is not ASCII
//!   (then the error carries the input and the table is unchanged);
//!   otherwise the symbol lives in the ASCII pool iff the mode is not
//!   `Utf8`, `resolve` and `resolve_str` return exactly the input,
//!   `find_symbol` returns the same symbol, re-interning is idempotent,
//!   `len` grows by one iff the string was new, and well-formedness and the
//!   "ASCII pool holds only ASCII" invariant are preserved.
//! * `theorem_intern_injective`: two successful interns in the same mode
//!   return equal symbols iff the inputs are equal.
//! * `theorem_find_symbol`: `find_symbol` returns `Some(sym)` iff the input
//!   was interned in that mode's pool, and then `sym` is what
//!   `intern_symbol` returns without changing the table.
//! * `theorem_checkpoint_restore`: for any sequence of `intern_symbol`
//!   calls after `checkpoint`, `restore` returns the table to exactly (map
//!   and vector, extensionally) the checkpointed state; ids of strings that
//!   were present at the checkpoint never change in between.
//! * `theorem_restore_rejects_newer_checkpoint`: `restore` panics (model:
//!   `None`) exactly when the checkpoint is longer than the current pools.
//!
//! # Model and trusted base
//!
//! * `FxHashMap<Box<[u8]>, u32>` / `FxHashMap<Box<str>, u32>` are modeled by
//!   `vstd::map::Map<Seq<u8>, u32>`, and `Vec<Box<[u8]>>` / `Vec<Box<str>>`
//!   by `Seq<Seq<u8>>`. A `str` is modeled by its UTF-8 bytes. Trusted: the
//!   standard `HashMap` (with `FxBuildHasher`) and `Vec` implement these
//!   mathematical maps/sequences, and `Box<str>`/`Box<[u8]>` equality and
//!   hashing are byte-content based.
//! * `intern_ascii` and `intern_utf8` have identical bodies over different
//!   pools; both are modeled by `pool_intern` plus the pool/tag selection.
//!   The two `while` loops of `restore` are both modeled by `pool_restore`.
//! * A panic (`assert!` in `restore`, out-of-range index in `resolve`) is
//!   modeled by `None`.
//! * `str::is_ascii` is modeled by `is_ascii` (every byte < 0x80) and
//!   `std::str::from_utf8(..).ok()` by `vstd::utf8::valid_utf8`.
//! * `EncodingError::NonAsciiSymbol(String)` is modeled by the input bytes.
//! * No `assume`, `admit`, `external_body` or trusted glue in this file.
//!
//! # Known gaps
//!
//! * `intern_*` computes the new id as `len as u32` (symbol.rs:129, 144,
//!   with `#[allow(clippy::cast_possible_truncation)]`). The theorems require
//!   each pool to hold fewer than `u32::MAX` strings before the operations
//!   (precondition `pool_has_room`); at 2^32 symbols the cast would wrap and
//!   ids would collide. This is a documented capacity limit, not modeled
//!   further.
//! * The model is not mechanically linked to the Rust source; fidelity is by
//!   the line-by-line correspondence noted on each function. Kani harnesses
//!   in crates/ferric-rules-core/src/symbol/kani_proofs.rs check the
//!   non-hashing parts on the production code.
//! * Serde deserialization of a `SymbolTable` (feature `serde`) can produce
//!   tables that violate `wf`; that path is validated by snapshot code and is
//!   out of scope here.

use vstd::prelude::*;
use vstd::utf8::*;

verus! {

/// Models `SymbolId` (crates/ferric-rules-core/src/symbol.rs:44 enum SymbolId).
pub enum SymbolId {
    Ascii(u32),
    Utf8(u32),
}

/// Models `StringEncoding` (crates/ferric-rules-core/src/encoding.rs:12).
pub enum StringEncoding {
    Ascii,
    Utf8,
    AsciiSymbolsUtf8Strings,
}

/// Models `Result<Symbol, EncodingError>` as returned by `intern_symbol`
/// (symbol.rs:210); the only error it produces is `NonAsciiSymbol(s)`.
pub enum InternResult {
    Ok(SymbolId),
    NonAsciiSymbol(Seq<u8>),
}

/// One interning pool: `ascii_to_id` + `ascii_strings`, or `utf8_to_id` +
/// `utf8_strings` (symbol.rs:58-64).
pub struct Pool {
    pub to_id: Map<Seq<u8>, u32>,
    pub strings: Seq<Seq<u8>>,
}

/// Models `SymbolTable` (symbol.rs:55 struct SymbolTable).
pub struct SymbolTable {
    pub ascii: Pool,
    pub utf8: Pool,
}

/// Models `SymbolTableCheckpoint` (symbol.rs:69).
pub struct Checkpoint {
    pub ascii_len: nat,
    pub utf8_len: nat,
}

/// Models `str::is_ascii`.
pub open spec fn is_ascii(s: Seq<u8>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> #[trigger] s[i] < 0x80
}

// ---------------------------------------------------------------------------
// Model of the production functions
// ---------------------------------------------------------------------------

/// Models `SymbolTable::new` (symbol.rs:77 fn new).
pub open spec fn new_table() -> SymbolTable {
    SymbolTable {
        ascii: Pool { to_id: Map::empty(), strings: Seq::empty() },
        utf8: Pool { to_id: Map::empty(), strings: Seq::empty() },
    }
}

/// Models `SymbolTable::checkpoint` (symbol.rs:88 fn checkpoint).
pub open spec fn checkpoint(t: SymbolTable) -> Checkpoint {
    Checkpoint { ascii_len: t.ascii.strings.len(), utf8_len: t.utf8.strings.len() }
}

/// Models the common body of `intern_ascii` (symbol.rs:125) and
/// `intern_utf8` (symbol.rs:140):
/// ```text
/// if let Some(&id) = self.X_to_id.get(s) { return id; }
/// let id = self.X_strings.len() as u32;
/// self.X_to_id.insert(boxed.clone(), id);
/// self.X_strings.push(boxed);
/// id
/// ```
pub open spec fn pool_intern(p: Pool, s: Seq<u8>) -> (Pool, u32) {
    if p.to_id.contains_key(s) {
        (p, p.to_id[s])
    } else {
        let id = p.strings.len() as u32;
        (Pool { to_id: p.to_id.insert(s, id), strings: p.strings.push(s) }, id)
    }
}

/// Models `SymbolTable::intern_ascii` (symbol.rs:125 fn intern_ascii).
pub open spec fn intern_ascii(t: SymbolTable, s: Seq<u8>) -> (SymbolTable, SymbolId) {
    let (p, id) = pool_intern(t.ascii, s);
    (SymbolTable { ascii: p, utf8: t.utf8 }, SymbolId::Ascii(id))
}

/// Models `SymbolTable::intern_utf8` (symbol.rs:140 fn intern_utf8).
pub open spec fn intern_utf8(t: SymbolTable, s: Seq<u8>) -> (SymbolTable, SymbolId) {
    let (p, id) = pool_intern(t.utf8, s);
    (SymbolTable { ascii: t.ascii, utf8: p }, SymbolId::Utf8(id))
}

/// Models one `while` loop of `SymbolTable::restore` (symbol.rs:105-118):
/// ```text
/// while self.X_strings.len() > checkpoint.X_len {
///     let symbol = self.X_strings.pop().expect(..);
///     self.X_to_id.remove(symbol.as_ref());
/// }
/// ```
pub open spec fn pool_restore(p: Pool, len: nat) -> Pool
    decreases p.strings.len(),
{
    if p.strings.len() > len {
        let symbol = p.strings.last();
        pool_restore(Pool { to_id: p.to_id.remove(symbol), strings: p.strings.drop_last() }, len)
    } else {
        p
    }
}

/// Models `SymbolTable::restore` (symbol.rs:99 fn restore); `None` models
/// the `assert!` panic for a checkpoint newer than the table.
pub open spec fn restore(t: SymbolTable, cp: Checkpoint) -> Option<SymbolTable> {
    if cp.ascii_len <= t.ascii.strings.len() && cp.utf8_len <= t.utf8.strings.len() {
        Some(
            SymbolTable {
                ascii: pool_restore(t.ascii, cp.ascii_len),
                utf8: pool_restore(t.utf8, cp.utf8_len),
            },
        )
    } else {
        None
    }
}

/// Models `SymbolTable::resolve` (symbol.rs:157 fn resolve); `None` models
/// the index-out-of-bounds panic.
pub open spec fn resolve(t: SymbolTable, id: SymbolId) -> Option<Seq<u8>> {
    match id {
        SymbolId::Ascii(i) => if (i as int) < t.ascii.strings.len() {
            Some(t.ascii.strings[i as int])
        } else {
            None
        },
        SymbolId::Utf8(i) => if (i as int) < t.utf8.strings.len() {
            Some(t.utf8.strings[i as int])
        } else {
            None
        },
    }
}

/// Models `SymbolTable::resolve_str` (symbol.rs:166 fn resolve_str):
/// the ASCII arm is `get(i).and_then(|b| from_utf8(b).ok())`.
pub open spec fn resolve_str(t: SymbolTable, id: SymbolId) -> Option<Seq<u8>> {
    match id {
        SymbolId::Ascii(i) => if (i as int) < t.ascii.strings.len() && valid_utf8(
            t.ascii.strings[i as int],
        ) {
            Some(t.ascii.strings[i as int])
        } else {
            None
        },
        SymbolId::Utf8(i) => if (i as int) < t.utf8.strings.len() {
            Some(t.utf8.strings[i as int])
        } else {
            None
        },
    }
}

/// Models `SymbolTable::len` (symbol.rs:178 fn len).
pub open spec fn len(t: SymbolTable) -> nat {
    t.ascii.strings.len() + t.utf8.strings.len()
}

/// Models `SymbolTable::intern_symbol` (symbol.rs:210 fn intern_symbol).
pub open spec fn intern_symbol(t: SymbolTable, s: Seq<u8>, enc: StringEncoding) -> (
    SymbolTable,
    InternResult,
) {
    match enc {
        StringEncoding::Ascii | StringEncoding::AsciiSymbolsUtf8Strings => {
            if !is_ascii(s) {
                (t, InternResult::NonAsciiSymbol(s))
            } else {
                let (t2, id) = intern_ascii(t, s);
                (t2, InternResult::Ok(id))
            }
        },
        StringEncoding::Utf8 => {
            let (t2, id) = intern_utf8(t, s);
            (t2, InternResult::Ok(id))
        },
    }
}

/// Models `SymbolTable::find_symbol` (symbol.rs:232 fn find_symbol).
pub open spec fn find_symbol(t: SymbolTable, s: Seq<u8>, enc: StringEncoding) -> Option<SymbolId> {
    match enc {
        StringEncoding::Ascii | StringEncoding::AsciiSymbolsUtf8Strings => {
            if !is_ascii(s) {
                None
            } else if t.ascii.to_id.contains_key(s) {
                Some(SymbolId::Ascii(t.ascii.to_id[s]))
            } else {
                None
            }
        },
        StringEncoding::Utf8 => {
            if t.utf8.to_id.contains_key(s) {
                Some(SymbolId::Utf8(t.utf8.to_id[s]))
            } else {
                None
            }
        },
    }
}

/// A sequence of `intern_symbol` calls (the only public mutator besides
/// `restore`).
pub struct Op {
    pub s: Seq<u8>,
    pub enc: StringEncoding,
}

pub open spec fn run(t: SymbolTable, ops: Seq<Op>) -> SymbolTable
    decreases ops.len(),
{
    if ops.len() == 0 {
        t
    } else {
        run(intern_symbol(t, ops[0].s, ops[0].enc).0, ops.drop_first())
    }
}

// ---------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------

/// The map and the vector of a pool are mutually inverse: the vector lists
/// each key once, at the index the map assigns to it.
pub open spec fn pool_wf(p: Pool) -> bool {
    &&& forall|i: int|
        0 <= i < p.strings.len() ==> #[trigger] p.to_id.contains_key(p.strings[i])
            && p.to_id[p.strings[i]] as int == i
    &&& forall|s: Seq<u8>|
        #[trigger] p.to_id.contains_key(s) ==> (p.to_id[s] as int) < p.strings.len()
            && p.strings[p.to_id[s] as int] == s
}

pub open spec fn wf(t: SymbolTable) -> bool {
    pool_wf(t.ascii) && pool_wf(t.utf8)
}

/// Every string in the ASCII pool is ASCII (the encoding invariant that
/// `intern_symbol` maintains for the ASCII pool).
pub open spec fn ascii_pool_is_ascii(t: SymbolTable) -> bool {
    forall|i: int| 0 <= i < t.ascii.strings.len() ==> #[trigger] is_ascii(t.ascii.strings[i])
}

/// The new id `len as u32` is exact (no truncation) for `extra` more strings.
pub open spec fn pool_has_room(p: Pool, extra: nat) -> bool {
    p.strings.len() + extra <= u32::MAX as nat
}

/// `p.strings` extends `base.strings`.
pub open spec fn extends(p: Pool, base: Pool) -> bool {
    &&& base.strings.len() <= p.strings.len()
    &&& forall|i: int| 0 <= i < base.strings.len() ==> #[trigger] p.strings[i] == base.strings[i]
}

// ---------------------------------------------------------------------------
// Pool lemmas
// ---------------------------------------------------------------------------

pub proof fn lemma_new_wf()
    ensures
        wf(new_table()),
        ascii_pool_is_ascii(new_table()),
        len(new_table()) == 0,
{
}

/// Interning into a well-formed pool.
pub proof fn lemma_pool_intern(p: Pool, s: Seq<u8>)
    requires
        pool_wf(p),
        pool_has_room(p, 1),
    ensures
        ({
            let (q, id) = pool_intern(p, s);
            &&& pool_wf(q)
            &&& extends(q, p)
            &&& (id as int) < q.strings.len()
            &&& q.strings[id as int] == s
            &&& q.to_id.contains_key(s) && q.to_id[s] == id
            &&& p.to_id.contains_key(s) ==> q == p
            &&& !p.to_id.contains_key(s) ==> id as int == p.strings.len() && q.strings.len()
                == p.strings.len() + 1
            &&& forall|k: Seq<u8>| #[trigger]
                p.to_id.contains_key(k) ==> q.to_id.contains_key(k) && q.to_id[k] == p.to_id[k]
        }),
{
    let (q, id) = pool_intern(p, s);
    if !p.to_id.contains_key(s) {
        let n = p.strings.len();
        assert(id as int == n as int);
        assert forall|i: int| 0 <= i < q.strings.len() implies #[trigger] q.to_id.contains_key(
            q.strings[i],
        ) && q.to_id[q.strings[i]] as int == i by {
            if i < n {
                assert(q.strings[i] == p.strings[i]);
                assert(p.to_id.contains_key(p.strings[i]));
                assert(p.strings[i] != s);
            } else {
                assert(q.strings[i] == s);
            }
        }
        assert forall|k: Seq<u8>| #[trigger] q.to_id.contains_key(k) implies (q.to_id[k] as int)
            < q.strings.len() && q.strings[q.to_id[k] as int] == k by {
            if k != s {
                assert(p.to_id.contains_key(k));
            }
        }
    }
}

/// Removing the last string of a well-formed pool keeps it well formed.
pub proof fn lemma_pool_pop(p: Pool)
    requires
        pool_wf(p),
        p.strings.len() > 0,
    ensures
        pool_wf(Pool { to_id: p.to_id.remove(p.strings.last()), strings: p.strings.drop_last() }),
{
    let n = p.strings.len() as int;
    let s = p.strings.last();
    let q = Pool { to_id: p.to_id.remove(s), strings: p.strings.drop_last() };
    assert(p.to_id.contains_key(p.strings[n - 1]));
    assert forall|i: int| 0 <= i < q.strings.len() implies #[trigger] q.to_id.contains_key(
        q.strings[i],
    ) && q.to_id[q.strings[i]] as int == i by {
        assert(q.strings[i] == p.strings[i]);
        assert(p.to_id.contains_key(p.strings[i]));
        assert(p.to_id[p.strings[i]] as int == i);
        assert(p.to_id[s] as int == n - 1);
        assert(p.strings[i] != s);
    }
    assert forall|k: Seq<u8>| #[trigger] q.to_id.contains_key(k) implies (q.to_id[k] as int)
        < q.strings.len() && q.strings[q.to_id[k] as int] == k by {
        assert(p.to_id.contains_key(k));
        assert(k != s);
        if p.to_id[k] as int == n - 1 {
            assert(p.strings[n - 1] == k);
        }
    }
}

/// A well-formed pool is determined by its vector.
pub proof fn lemma_pool_wf_unique(p: Pool, q: Pool)
    requires
        pool_wf(p),
        pool_wf(q),
        p.strings == q.strings,
    ensures
        p == q,
{
    assert forall|k: Seq<u8>| #[trigger] p.to_id.contains_key(k) implies q.to_id.contains_key(k)
        by {
        let i = p.to_id[k] as int;
        assert(p.strings[i] == k);
        assert(q.to_id.contains_key(q.strings[i]));
    }
    assert forall|k: Seq<u8>| #[trigger] q.to_id.contains_key(k) implies p.to_id.contains_key(k)
        by {
        let i = q.to_id[k] as int;
        assert(q.strings[i] == k);
        assert(p.to_id.contains_key(p.strings[i]));
    }
    assert forall|k: Seq<u8>| #[trigger] p.to_id.contains_key(k) implies q.to_id[k] == p.to_id[k]
        by {
        let i = p.to_id[k] as int;
        assert(p.strings[i] == k);
        assert(q.to_id.contains_key(q.strings[i]));
        assert(q.to_id[q.strings[i]] as int == i);
    }
    assert(p.to_id.dom() =~= q.to_id.dom());
    assert(p.to_id =~= q.to_id);
}

/// `pool_restore` returns exactly the well-formed pool it extends.
pub proof fn lemma_pool_restore(p: Pool, base: Pool)
    requires
        pool_wf(p),
        pool_wf(base),
        extends(p, base),
    ensures
        pool_restore(p, base.strings.len()) == base,
    decreases p.strings.len(),
{
    if p.strings.len() > base.strings.len() {
        let q = Pool { to_id: p.to_id.remove(p.strings.last()), strings: p.strings.drop_last() };
        lemma_pool_pop(p);
        assert(extends(q, base));
        lemma_pool_restore(q, base);
    } else {
        assert(p.strings =~= base.strings);
        lemma_pool_wf_unique(p, base);
    }
}

/// ASCII bytes are valid UTF-8, so `from_utf8` succeeds on them.
pub proof fn lemma_ascii_is_valid_utf8(s: Seq<u8>)
    requires
        is_ascii(s),
    ensures
        valid_utf8(s),
{
    assert(s.subrange(0, 0) =~= Seq::<u8>::empty());
    assert(partial_valid_utf8(s, 0));
    assert forall|i: int| 0 <= 0 <= i < s.len() <= s.len() implies #[trigger] is_leading_byte_width_1(
        s[i],
    ) by {
        assert(s[i] < 0x80);
    }
    partial_valid_utf8_extend_ascii_block(s, 0, s.len() as int);
    assert(s.subrange(0, s.len() as int) =~= s);
}

// ---------------------------------------------------------------------------
// Theorems about the public API
// ---------------------------------------------------------------------------

/// Exact characterization of `intern_symbol`.
pub proof fn theorem_intern_symbol(t: SymbolTable, s: Seq<u8>, enc: StringEncoding)
    requires
        wf(t),
        ascii_pool_is_ascii(t),
        pool_has_room(t.ascii, 1),
        pool_has_room(t.utf8, 1),
    ensures
        ({
            let (t2, r) = intern_symbol(t, s, enc);
            let ascii_mode = enc != StringEncoding::Utf8;
            &&& wf(t2)
            &&& ascii_pool_is_ascii(t2)
            &&& extends(t2.ascii, t.ascii)
            &&& extends(t2.utf8, t.utf8)
            &&& (r is NonAsciiSymbol <==> ascii_mode && !is_ascii(s))
            &&& (r is NonAsciiSymbol ==> r->NonAsciiSymbol_0 == s && t2 == t)
            &&& (r is Ok ==> {
                let id = r->Ok_0;
                &&& (id is Ascii <==> ascii_mode)
                &&& resolve(t2, id) == Some(s)
                &&& resolve_str(t2, id) == Some(s)
                &&& find_symbol(t2, s, enc) == Some(id)
                &&& intern_symbol(t2, s, enc) == (t2, InternResult::Ok(id))
                &&& (find_symbol(t, s, enc) is Some ==> t2 == t && len(t2) == len(t))
                &&& (find_symbol(t, s, enc) is None ==> len(t2) == len(t) + 1)
            })
        }),
{
    let (t2, r) = intern_symbol(t, s, enc);
    match enc {
        StringEncoding::Ascii | StringEncoding::AsciiSymbolsUtf8Strings => {
            if is_ascii(s) {
                lemma_pool_intern(t.ascii, s);
                assert forall|i: int| 0 <= i < t2.ascii.strings.len() implies #[trigger] is_ascii(
                    t2.ascii.strings[i],
                ) by {
                    if i < t.ascii.strings.len() {
                        assert(t2.ascii.strings[i] == t.ascii.strings[i]);
                    } else {
                        assert(t2.ascii.strings[i] == s);
                    }
                }
                lemma_ascii_is_valid_utf8(s);
            }
        },
        StringEncoding::Utf8 => {
            lemma_pool_intern(t.utf8, s);
        },
    }
}

/// Interning is injective per mode: equal symbols iff equal inputs.
pub proof fn theorem_intern_injective(
    t: SymbolTable,
    s1: Seq<u8>,
    s2: Seq<u8>,
    enc: StringEncoding,
)
    requires
        wf(t),
        ascii_pool_is_ascii(t),
        pool_has_room(t.ascii, 2),
        pool_has_room(t.utf8, 2),
    ensures
        ({
            let (t1, r1) = intern_symbol(t, s1, enc);
            let (t2, r2) = intern_symbol(t1, s2, enc);
            r1 is Ok && r2 is Ok ==> (r1->Ok_0 == r2->Ok_0 <==> s1 == s2)
        }),
{
    theorem_intern_symbol(t, s1, enc);
    let (t1, r1) = intern_symbol(t, s1, enc);
    theorem_intern_symbol(t1, s2, enc);
    let (t2, r2) = intern_symbol(t1, s2, enc);
    if r1 is Ok && r2 is Ok {
        // t1's entries survive in t2, so resolving r1's id in t2 still gives s1.
        assert(resolve(t1, r1->Ok_0) == Some(s1));
        match r1->Ok_0 {
            SymbolId::Ascii(i) => {
                assert(t2.ascii.strings[i as int] == t1.ascii.strings[i as int]);
            },
            SymbolId::Utf8(i) => {
                assert(t2.utf8.strings[i as int] == t1.utf8.strings[i as int]);
            },
        }
        assert(resolve(t2, r1->Ok_0) == Some(s1));
        assert(resolve(t2, r2->Ok_0) == Some(s2));
    }
}

/// `find_symbol` returns `Some` exactly for strings already interned in the
/// mode's pool, and then agrees with a non-mutating `intern_symbol`.
pub proof fn theorem_find_symbol(t: SymbolTable, s: Seq<u8>, enc: StringEncoding)
    requires
        wf(t),
        ascii_pool_is_ascii(t),
        pool_has_room(t.ascii, 1),
        pool_has_room(t.utf8, 1),
    ensures
        find_symbol(t, s, enc) is Some <==> (enc == StringEncoding::Utf8 || is_ascii(s)) && (if enc
            == StringEncoding::Utf8 {
            t.utf8.to_id.contains_key(s)
        } else {
            t.ascii.to_id.contains_key(s)
        }),
        find_symbol(t, s, enc) is Some ==> intern_symbol(t, s, enc) == (
            t,
            InternResult::Ok(find_symbol(t, s, enc)->Some_0),
        ),
{
    theorem_intern_symbol(t, s, enc);
}

/// Each `intern_symbol` in a sequence preserves the invariants and only
/// extends the pools.
pub proof fn lemma_run(t: SymbolTable, ops: Seq<Op>)
    requires
        wf(t),
        ascii_pool_is_ascii(t),
        pool_has_room(t.ascii, ops.len()),
        pool_has_room(t.utf8, ops.len()),
    ensures
        wf(run(t, ops)),
        ascii_pool_is_ascii(run(t, ops)),
        extends(run(t, ops).ascii, t.ascii),
        extends(run(t, ops).utf8, t.utf8),
        run(t, ops).ascii.strings.len() <= t.ascii.strings.len() + ops.len(),
        run(t, ops).utf8.strings.len() <= t.utf8.strings.len() + ops.len(),
    decreases ops.len(),
{
    if ops.len() > 0 {
        let t1 = intern_symbol(t, ops[0].s, ops[0].enc).0;
        theorem_intern_symbol(t, ops[0].s, ops[0].enc);
        assert(t1.ascii.strings.len() <= t.ascii.strings.len() + 1) by {
            lemma_pool_intern(t.ascii, ops[0].s);
        }
        assert(t1.utf8.strings.len() <= t.utf8.strings.len() + 1) by {
            lemma_pool_intern(t.utf8, ops[0].s);
        }
        lemma_run(t1, ops.drop_first());
    }
}

/// Checkpoint/restore: `restore` after any sequence of `intern_symbol`
/// calls returns exactly the checkpointed table.
pub proof fn theorem_checkpoint_restore(t: SymbolTable, ops: Seq<Op>)
    requires
        wf(t),
        ascii_pool_is_ascii(t),
        pool_has_room(t.ascii, ops.len()),
        pool_has_room(t.utf8, ops.len()),
    ensures
        restore(run(t, ops), checkpoint(t)) == Some(t),
        // Ids of strings present at the checkpoint are stable in between.
        forall|i: u32|
            #![trigger resolve(t, SymbolId::Ascii(i))]
            resolve(t, SymbolId::Ascii(i)) is Some ==> resolve(run(t, ops), SymbolId::Ascii(i))
                == resolve(t, SymbolId::Ascii(i)),
        forall|i: u32|
            #![trigger resolve(t, SymbolId::Utf8(i))]
            resolve(t, SymbolId::Utf8(i)) is Some ==> resolve(run(t, ops), SymbolId::Utf8(i))
                == resolve(t, SymbolId::Utf8(i)),
{
    lemma_run(t, ops);
    let t2 = run(t, ops);
    lemma_pool_restore(t2.ascii, t.ascii);
    lemma_pool_restore(t2.utf8, t.utf8);
}

/// `restore` panics exactly for a checkpoint newer than the table.
pub proof fn theorem_restore_rejects_newer_checkpoint(t: SymbolTable, cp: Checkpoint)
    ensures
        restore(t, cp) is None <==> (cp.ascii_len > t.ascii.strings.len() || cp.utf8_len
            > t.utf8.strings.len()),
{
}

/// Non-vacuity witness: the theorems' preconditions hold for a concrete
/// table built from `new`, and the characterization yields the expected
/// concrete ids (the two pools number independently from 0).
pub proof fn example_concrete_interning() {
    let a = seq![0x61u8];
    let b = seq![0x62u8];
    let e_acute = seq![0xC3u8, 0xA9u8];
    let t0 = new_table();
    lemma_new_wf();

    theorem_intern_symbol(t0, a, StringEncoding::Ascii);
    let (t1, r1) = intern_symbol(t0, a, StringEncoding::Ascii);
    assert(r1 == InternResult::Ok(SymbolId::Ascii(0)));

    theorem_intern_symbol(t1, b, StringEncoding::AsciiSymbolsUtf8Strings);
    let (t2, r2) = intern_symbol(t1, b, StringEncoding::AsciiSymbolsUtf8Strings);
    assert(r2 == InternResult::Ok(SymbolId::Ascii(1)));

    assert(!is_ascii(e_acute)) by {
        assert(e_acute[0] >= 0x80);
    }
    theorem_intern_symbol(t2, e_acute, StringEncoding::Ascii);
    assert(intern_symbol(t2, e_acute, StringEncoding::Ascii).1 == InternResult::NonAsciiSymbol(
        e_acute,
    ));

    theorem_intern_symbol(t2, e_acute, StringEncoding::Utf8);
    let (t3, r3) = intern_symbol(t2, e_acute, StringEncoding::Utf8);
    assert(r3 == InternResult::Ok(SymbolId::Utf8(0)));
    assert(len(t3) == 3);

    let ops = seq![
        Op { s: a, enc: StringEncoding::Ascii },
        Op { s: e_acute, enc: StringEncoding::Utf8 },
        Op { s: seq![0x63u8], enc: StringEncoding::Utf8 },
    ];
    theorem_checkpoint_restore(t3, ops);
    assert(restore(run(t3, ops), checkpoint(t3)) == Some(t3));
}

} // verus!
