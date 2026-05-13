# Third-party Materials

Except where otherwise noted, Ferric is licensed under `MIT OR Apache-2.0`.
See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Cargo Dependencies

Cargo license notices are generated from the locked Cargo workspace graph in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The generated file includes
Ferric workspace crates as well as third-party Cargo crates. Regenerate it with:

```sh
just license-notices
```

Check that the committed notices match the current generated output with:

```sh
just license-notices-check
```

## Compatibility Test Data

The `tests/examples/` tree contains third-party CLIPS programs and related
compatibility fixtures used for parser and engine validation. Those files are
not relicensed under Ferric's `MIT OR Apache-2.0` license. They retain their
original upstream licenses and notices.

Known license families in that corpus include GPLv2, GPLv2-or-later, GPLv3,
BSD-style terms, and Apache-2.0 terms. In particular, GPL notices are present
under paths such as:

- `tests/examples/fawkes-robotics/`
- `tests/examples/labcegor/`
- `tests/examples/learn-clips/`
- `tests/examples/rcll-refbox/`

Do not treat `tests/examples/` as an MIT/Apache-only corpus when preparing
source archives or downstream redistributions with stricter licensing
requirements.
