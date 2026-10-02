use jujutsu_mcp::jj::JjOutput;
use jujutsu_mcp::tools::text_result;

fn texts(output: JjOutput) -> (Option<bool>, Vec<String>) {
    let result = text_result(output);
    let blocks = result
        .content
        .iter()
        .map(|block| block.as_text().expect("text block").text.clone())
        .collect();
    (result.is_error, blocks)
}

#[test]
fn stdout_only() {
    let (is_error, blocks) = texts(JjOutput {
        stdout: "out\n".to_owned(),
        stderr: String::new(),
    });
    assert_ne!(is_error, Some(true));
    assert_eq!(blocks, ["out\n"]);
}

#[test]
fn stderr_follows_stdout() {
    let (_, blocks) = texts(JjOutput {
        stdout: "out\n".to_owned(),
        stderr: "Working copy now at: abc\n".to_owned(),
    });
    assert_eq!(blocks, ["out\n", "Working copy now at: abc\n"]);
}

#[test]
fn only_stderr_still_reports_it() {
    let (_, blocks) = texts(JjOutput {
        stdout: String::new(),
        stderr: "Nothing changed.\n".to_owned(),
    });
    assert!(
        blocks.iter().any(|b| b == "Nothing changed.\n"),
        "{blocks:?}"
    );
}
