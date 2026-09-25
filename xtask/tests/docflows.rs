// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a documented `yaml` fence becomes the workflow a reader would paste.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking"
)]

fn shaped(fence: &str) -> String {
    let root = tempfile::tempdir().expect("a scratch documentation root");
    std::fs::create_dir_all(root.path().join("docs")).expect("docs");
    std::fs::write(root.path().join("README.md"), "").expect("readme");
    std::fs::write(
        root.path().join("docs/page.md"),
        format!("# Page\n\n```yaml\n{fence}```\n"),
    )
    .expect("page");
    let found = xtask::docflows::snippets(root.path()).expect("the page reads");
    assert_eq!(found.len(), 1, "one fence, one snippet: {found:?}");
    assert_eq!(
        found[0].line, 3,
        "the fence is named by the line it opens on"
    );
    found[0].workflow.clone()
}

#[test]
fn a_list_of_steps_is_a_job_a_reader_pastes_them_into() {
    let workflow = shaped("- run: njutest verify\n");
    assert!(
        workflow.starts_with("on: push\njobs:\n  snippet:\n")
            && workflow.contains("    steps:\n      - run: njutest verify\n"),
        "steps on their own are linted as the steps of one job: {workflow}"
    );
}

#[test]
fn this_repository_s_actions_are_linted_as_the_actions_this_commit_ships() {
    let workflow = shaped(
        "jobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: <owner>/njutest/.github/actions/njutest@v0.1.0\n",
    );
    assert!(
        workflow.contains("uses: ./.github/actions/njutest\n")
            && workflow.starts_with("on: push\n"),
        "a documented `uses:` of this repository's action is checked against the inputs it declares \
         today, and a workflow without a trigger is given one: {workflow}"
    );
}

#[test]
fn a_workflow_that_names_its_trigger_keeps_it() {
    let workflow = shaped("name: x\non:\n  pull_request:\njobs: {}\n");
    assert!(
        !workflow.starts_with("on: push"),
        "a trigger the page wrote is the one linted: {workflow}"
    );
}

#[test]
fn a_documented_action_this_repository_does_not_ship_is_refused_by_name() {
    let root = tempfile::tempdir().expect("a scratch repository");
    std::fs::create_dir_all(root.path().join("docs")).expect("docs");
    std::fs::create_dir_all(root.path().join(".github/actions/shipped")).expect("an action");
    std::fs::write(
        root.path().join(".github/actions/shipped/action.yml"),
        "name: shipped\ndescription: x\nruns:\n  using: composite\n  steps: []\n",
    )
    .expect("action.yml");
    std::fs::write(root.path().join("README.md"), "").expect("readme");
    std::fs::write(
        root.path().join("docs/page.md"),
        "# Page\n\n```yaml\n- uses: P4suta/njutest/.github/actions/shipped@main\n- uses: \
         P4suta/njutest/.github/actions/missing@main\n```\n",
    )
    .expect("page");
    let refused = xtask::docflows::check(root.path(), std::ffi::OsStr::new("actionlint"));
    let Err(xtask::docflows::DocflowsError::Refused(said)) = refused else {
        panic!("a page that names an action nobody ships passes: {refused:?}");
    };
    assert!(
        said.contains("actions/missing") && !said.contains("actions/shipped"),
        "the refusal names the missing action and only it: {said}"
    );
}
