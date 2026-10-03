// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// The shell loop a scripted run waits in until `$TURNS/go` appears, which gives up once `$TURNS` is gone, as it is when the test ends however it ends, and after two minutes whatever happened, so no worker outlives the test that started it.
const UNTIL_GO: &str = "i=0; while [ ! -e \"$TURNS/go\" ] && [ -d \"$TURNS\" ] && [ \"$i\" -lt 2400 ]; do sleep 0.05; i=$((i + 1)); done";
