# Pass 004: CLIPS Compatibility Language, Module, And Stdlib Semantics Suite

## Objective

Complete compatibility coverage for Phase 3/4 language closures and documented standard-library behavior.

## Scope

- Modules/globals/function visibility and qualified-name semantics.
- Generic dispatch (`call-next-method`, specificity ordering, conflict diagnostics).
- Documented stdlib families and I/O/environment behavior.

## Tasks

1. Add compatibility fixtures for module-qualified global/function behavior, visibility boundaries, and ambiguity diagnostics.
2. Add compatibility fixtures for generic dispatch ordering and `call-next-method` chaining behavior.
3. Add compatibility fixtures for documented stdlib families (predicate/math/string/symbol/multifield/io/environment/fact/agenda).
4. Validate canonical `?*MODULE::name*` global syntax and `bind` non-creation behavior in compatibility assertions.
5. Ensure source-located diagnostics remain stable in compatibility outputs for unsupported/invalid forms.

## Definition Of Done

- Language/module/generic/stdlib compatibility coverage is comprehensive for supported features.
- Compatibility assertions align with current documented semantics and diagnostics.

## Verification Commands

- `cargo test --test clips_compat_language`
- `cargo test --workspace`

## Handoff State

- CLIPS compatibility suite now covers both core execution and higher-level language/library semantics.
