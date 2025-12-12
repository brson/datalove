use std::path::Path;
use datalove_repl as repl;

/// Environment binding (name, type, value).
#[derive(serde::Serialize)]
struct EnvBinding {
    name: String,
    ty: String,
    value: String,
}

/// Test output for a single input to the REPL engine.
#[derive(serde::Serialize)]
struct InputResult {
    input: String,
    parse: repl::InputParse,
    eval: Option<repl::Eval>,
    environment: Vec<EnvBinding>,
}

/// Process a REPL fixture file.
///
/// The file contains multiple inputs separated by a line containing only "---".
/// For each input, we record the parse result, eval result, and environment.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = repl::datafun::Database::default();
    let mut engine = repl::Engine::new(&db)
        .map_err(|e| format!("Failed to create engine: {}", e))?;

    let mut results = Vec::new();

    // Split the file into sections by "---" separator.
    let sections: Vec<&str> = source_text.split("\n---\n").collect();

    for section in sections {
        let input = section.trim();
        if input.is_empty() {
            continue;
        }

        // Detect if input contains newlines and use appropriate input type.
        let repl_input = if input.contains('\n') {
            repl::Input::Multiline(input.to_string())
        } else {
            repl::Input::Input(input.to_string())
        };

        let parse_result = engine.parse_input(repl_input);

        let eval_result = match &parse_result {
            repl::InputParse::Command(cmd) => {
                Some(engine.eval(cmd.clone()))
            }
            _ => None,
        };

        let environment = engine.get_environment()
            .into_iter()
            .map(|(name, ty, value)| EnvBinding { name, ty, value })
            .collect();

        results.push(InputResult {
            input: input.to_string(),
            parse: parse_result,
            eval: eval_result,
            environment,
        });
    }

    // Serialize results to pretty JSON.
    serde_json::to_string_pretty(&results)
        .map_err(|e| format!("Failed to serialize results: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("engine")
        .file_extension("repl")
        .run();
}
