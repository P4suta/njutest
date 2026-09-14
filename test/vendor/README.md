<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Vendored schemas

Third-party documents this repository validates its own output against. They
are vendored so the test that checks a projection needs no network, and so a
change in somebody else's repository cannot silently change what this one
claims to produce.

| File | From | Fetched | Licence |
| --- | --- | --- | --- |
| `mutation-testing-report-schema.json` | `https://raw.githubusercontent.com/stryker-mutator/mutation-testing-elements/master/packages/report-schema/src/mutation-testing-report-schema.json` | 2026-09-05 | Apache-2.0, © Stryker mutator team |

Each file is used verbatim. To refresh one, fetch it again, put the date in
the table, and let the test say whether what this repository writes still
validates.
