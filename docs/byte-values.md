# Byte values and instance names

Ferric preserves the bytes of STRING, SYMBOL, and INSTANCE-NAME values across
facts, indexing, captured output, snapshots, and embedding adapters. An instance
name is a distinct atom: `[widget]`, symbol `widget`, and string `"widget"` are
unequal types. Name values do not create COOL objects or enable instance slots,
message handlers, or instance addresses.

Use `instance-namep` to inspect a name's value type. CLIPS `type` instead looks
up the named object and returns its class. Since Ferric does not create COOL
objects, that lookup reports a missing instance and halts evaluation. A generic
method's typed restriction similarly requires object lookup when it examines a
name argument; a parameter with no type restriction can accept the name value.
The host `Value::type_name()` accessor inspects the value tag without object
lookup.

This value-model prerequisite does not itself repair `string-to-field`,
`explode$`, `read`, or `format`. Those functions require their own scanner and
formatting changes. In particular, the legacy scanners report invalid UTF-8
input instead of replacing bytes while that work remains pending.
The source parser still accepts text only: `save-facts` preserves byte values,
but a saved file containing invalid UTF-8 cannot yet be read by `load-facts`.
Dynamic symbol/name conversions accept either atom type, as CLIPS does. Ferric
does not reproduce CLIPS's additional static rejection of some literal
conversion arguments during source loading.

## Rust migration

```rust
use ferric_rules::runtime::{Engine, EngineConfig, HostValue};

let mut engine = Engine::new(EngineConfig::default());
let string = engine.create_string_bytes(b"a\0\xff")?;
assert_eq!(string.as_bytes(), b"a\0\xff");
assert!(string.as_str().is_err());
let symbol = engine.symbol_value_bytes(b"name\xff")?;
let instance = engine.instance_name_value_bytes(b"name\xff")?;
engine.assert_ordered("payload", vec![HostValue::from(string), symbol, instance])?;
# Ok::<(), ferric_rules::runtime::EngineError>(())
```

`FerricString::as_str()` now returns a decoding result, and
`Engine::get_output(channel)` returns `Result<Option<&str>, Utf8Error>`.
Propagate that result with `?` when text is required. Use `as_bytes()` and
`get_output_bytes(channel)` for byte data. The `FerricString` `AsRef<str>` and
`Borrow<str>` implementations are replaced by their byte-slice counterparts.
Display formatting escapes invalid bytes for inspection; output routing writes
the original bytes.

Existing text constructors keep their validation. Default `Utf8` mode accepts
arbitrary bytes through explicit byte APIs; it no longer implies that every
stored value is valid Unicode. `Ascii` continues rejecting non-ASCII strings,
symbols, and names. `AsciiSymbolsUtf8Strings` permits byte STRINGs while keeping
symbols and names ASCII-only. No constructor performs replacement decoding.

`InstanceNameHandle` retains engine ownership just like `SymbolHandle`.
`HostValue` rejects foreign or unowned interned atoms, including inside nested
multifields. Names survive reset and become invalid after clear or restoration.
Use owned values exported by the same engine for reassertion. Byte symbol and
instance-name resolution returns the unbracketed payload.

Snapshot schema 6 stores byte pools, typed names, and output bytes. Earlier
schema numbers are rejected before decoding; use their producing version to
recover application data. Schema numbers 2–5 are reserved by other compatibility
branches. A future combined snapshot layout needs a new schema number.

## C interface

The `FerricValue` layout and existing tags 0–6 are unchanged. The appended tags
are transport representations:

| Tag | CLIPS type | Active payload |
| --- | --- | --- |
| `STRING_BYTES` (7) | STRING | `string_ptr`, `multifield_len` bytes |
| `SYMBOL_BYTES` (8) | SYMBOL | `string_ptr`, `multifield_len` bytes |
| `INSTANCE_NAME` (9) | INSTANCE-NAME | Unbracketed `string_ptr`, `multifield_len` bytes |

There is no trailing NUL promise for these spans. Empty owned spans have a null
pointer and zero length. Only `ferric_value_free` releases them;
`ferric_string_free` is for legacy C-string allocations. Structured assertions
borrow spans for the duration of the call; multifield copying owns its complete
recursive copy. Existing UTF-8/NUL-free String/Symbol values retain old tags on
query, even if they entered through an explicit raw constructor.

Use `ferric_value_string_raw`, `ferric_value_symbol_raw`, and
`ferric_value_instance_name` to construct byte values. Existing `_bytes` text
constructors retain their UTF-8 and embedded-NUL checks. The new constructors
check null/length consistency and leave the output Void on failure.

`ferric_engine_get_output_copy` preserves raw bytes and reports a length that
includes its additional trailing NUL. Its size query and partial-buffer error
contract are unchanged. The legacy borrowed text output accessor rejects
invalid UTF-8 and embedded NUL. Do not use `strlen` to determine copied size.

## Language bindings

Python `ferric.String`, `ferric.Symbol`, and the new `ferric.InstanceName` retain
bytes internally and expose `.bytes`. Their `.value` and string conversion
check UTF-8. `Engine.get_output_bytes` retrieves complete output without a
Unicode conversion.

Go adds `StringBytes`, `SymbolBytes`, and `InstanceName`; their underlying Go
strings preserve arbitrary bytes. Explicit raw wrappers bypass legacy text
conversion. Swift uses `.stringBytes(Data)`, `.symbolBytes(Data)`, and
`.instanceName(Data)`, with `outputBytes` for captured output.

Node adds `FerricStringBytes`, `FerricSymbolBytes`, and `FerricInstanceName`
with byte-array payloads, plus `getOutputBytes`. Worker and pool transports keep
those types and bytes. Wire formats use explicit byte payloads rather than JSON
Unicode replacement. Text-only output views contain only decoded text; byte
output views retain the complete channel data.

See each binding's README for constructor syntax and error behavior. These are
value and transport changes; existing unsupported engine features retain their
own boundaries.
