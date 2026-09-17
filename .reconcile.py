import pathlib
import re

p = pathlib.Path("crates/njutest-cli/src/app/verify.rs")
s = p.read_text()

old = """    let request = asking(establishing, shard);
    let result = {
        let mut notes = ui::Notes::of(arguments.ui, stderr);
        run::run(&request, environment, &mut notes, watch)
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            trace.run_end("ERROR", None, Some(error.to_string()));
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };

    let report = outcome.report;"""
new = """    let request = asking(establishing, shard);
    let (report, kept) = match reconciled(&request, establishing, stderr) {
        Ok(both) => both,
        Err(code) => return code,
    };"""
assert old in s
s = s.replace(old, new, 1)
s = s.replace("            kept: &outcome.kept,", "            kept: &kept,", 1)

mine = pathlib.Path("/tmp/mine-verify.rs").read_text()
start = mine.index("/// What every build the configuration named establishes, as one report and what the run kept.")
end = mine.index("/// Where a run's engine recording goes")
block = mine[start:end]

s = s.replace(
    "/// Everything one run is asking for, gathered from the arguments, the configuration and the store.",
    block + "/// Everything one run is asking for, gathered from the arguments, the configuration and the store.",
    1,
)
p.write_text(s)
print("spliced,", len(block.splitlines()), "lines")
