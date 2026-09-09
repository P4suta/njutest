<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Reading a run back

**Status: implemented.** A run writes one document, and `rust-mutants report`
projects it into whatever a reader already has. Every projection is derived
from the stored `run-report-v1.json` and nothing else, so a projection is
never a second measurement — it is the same run said in another vocabulary.

```console
$ rust-mutants report --format markdown >> "$GITHUB_STEP_SUMMARY"
$ rust-mutants report --format junit --output target/mutants.xml
$ rust-mutants report --format sarif --output target/mutants.sarif
$ rust-mutants report --format html --output target/mutants.html
```

`--run ID` names a stored run; without it the newest is read. `--output FILE`
writes to a file and says where it put it. The exit code is the run's own, so
a projection in a pipeline still reports what the run established.

## The formats

| `--format` | What it is | Who reads it |
| --- | --- | --- |
| `lines` | what a person reads at a terminal, the default | you |
| `json` | the stored document, verbatim | a program of your own |
| `markdown` | score, accounting, findings, and a table of survivors | a pull request, a job summary |
| `junit` | one test case per mutant, one suite per file | a CI server's test view |
| `sarif` | SARIF 2.1.0, one result per finding | a code scanning view |
| `html` | one self-contained page with the source | a person, offline |
| `stryker` | the mutation testing report schema | a Stryker reader |

## What each one calls an outcome

JUnit has four states and this engine has six, so the mapping is a decision
rather than a translation:

| Outcome | JUnit | SARIF | Why |
| --- | --- | --- | --- |
| killed, timed out | a passing case | not a result | the tests noticed it, which is the tests working |
| survived | `<failure>` | `warning` | a gap in the tests is what a failing case means to a reader |
| inconclusive, errored | `<error>` | `error` | the run established nothing, which is about the run |
| not run | `<skipped>` with the reason | `note` | unreached, discharged, unselected, or stopped early |

SARIF carries a result **only** for a finding. A killed mutant is not a
finding, and a report full of results for code that is fine is a report nobody
reads. Each result carries a `partialFingerprints` entry holding the mutant's
identity, so a code scanning view can tell one run's finding from the next
run's.

A finding that is not about a mutant the report still holds — a claim nothing
answers to, a marker that hides nothing — has no place in the code to point
at. It is still a result, with no location rather than an invented one, and in
JUnit it is a failing case in a `findings` suite of its own. A finding nobody
can see in the view they actually read is a finding that does not exist.

## A score that falls because the measurement grew

A score is `detected / decided`, and `decided` counts only what the run
answered about. A mutation nothing measured reaches is `not_run` with
`unreached`, which says the measurement never asked — so it is outside the
ratio entirely.

That makes one movement look like a regression when it is the opposite. Write
a test that drives code no measurement reached before, and mutations of that
code stop being `unreached` and become answers: some killed, some survived.
The survivors were always there. What changed is that the run can now say so.
Measured on this workspace: one test added to a runner's own suite moved four
hundred and fifty eight mutations from unreached to executed, and the
survivors went from six to three hundred and five.

So a falling score with a rising `executed` is coverage arriving, not
detection leaving. The two numbers next to each other say which happened, and
a report that quotes the score alone cannot.

## The page

`--format html` writes one file that fetches nothing: no font, no stylesheet,
no image, and one script of its own that filters, searches and sorts the
table. Under the table it shows **every file that holds something to look at,
whole**, with each mutant on the line it is on and survivors coloured apart
from kills. A file whose every mutation the tests noticed is counted rather
than printed — a page that shows a thousand lines nobody has to read is a page
nobody opens — and its rows are in the table all the same. Then the candidates
the compiler refused, and the places discovery passed over.

The page shows a file only when it is the one the run measured, which the
recorded `source_digest` settles. A file that changed since is named as
changed rather than shown, because showing the new bytes would be a lie about
what was measured. A file that is not under `--root` at all is `RM0012`: a
projection that quietly left it out would read as a run that had nothing to
say about it.

## Thresholds

There are none, and there is no flag for one. A survivor is a finding, the
gate is an expectation with a reason, and `score.value` is in the JSON for
anyone who wants to draw a graph; see
[ADR 0004](../adr/0004-proof-layers-not-budgets.md). `[reports.stryker] high`
and `low` exist because that schema requires them and its readers colour by
them — nothing in this engine reads them back.

## Reading it at the terminal

`report --tui` browses the stored run instead of writing it. The rows are on
the left; on the right is whichever of the mutant, the file it is in, the
findings, or the keys you asked for.

| Key | What it does |
| --- | --- |
| `j` `k`, `↓` `↑` | the next mutant, the previous one |
| `PgDn` `PgUp`, `Home` `End` | a page at a time, the first, the last |
| `/` | search the path, rule, family, identity and outcome |
| `f` | cycle the outcome filter |
| `1`–`5` | all, survived, killed, not run, errored |
| `r` `p` `F` `?` | the mutant, its source, the findings, these keys |
| `y` | take the identity away with you |
| `q`, `Esc` | put the pane away, then quit |

`y` names the mutant you were on, and quitting prints that identity on
standard output, so `rust-mutants report --tui` in a command substitution
hands the next command a mutant a person chose:

```console
$ rust-mutants explain "$(rust-mutants report --tui)"
```

The source pane shows the file the run measured, which the recorded digest
settles. A file that changed since says so rather than showing what it holds
now.
