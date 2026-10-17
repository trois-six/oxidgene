# Conformance fixtures

GEDCOM files that exercise every structure of the standard: the "every
structure" conformance files of the oxidged project, copied unchanged.

- Source: https://github.com/trois-six/oxidged, path `tests/fixtures/conformance/`
- Commit: `2f737ac0a4c7aee622f7c98838db14c19e3d41a8`
- Files: `maximal551.ged` (GEDCOM 5.5.1), `full551.ged` (GEDCOM 5.5.1),
  `full70.ged` (GEDCOM 7.0), `extensions70.ged` (GEDCOM 7.0 with extension
  tags)
- The data is fictitious: every name, place and address is invented.
- Licence: MIT, copyright the oxidged authors, as recorded in the oxidged
  `LICENSES.md` ("Project-authored (MIT)"). The licence text follows.

`known_gaps.tsv` is not a fixture: it lists the losses the round-trip test
accepts, and is maintained with the test (see `tests/conformance_round_trip.rs`).

## Licence

MIT License

Copyright (c) 2020 Robert Pirtle
Copyright (c) 2026 Pierre Erraud and the oxidged contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
