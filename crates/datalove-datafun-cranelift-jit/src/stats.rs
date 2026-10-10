//! What the jit compiled and how calls crossed between it and the interpreter.
//!
//! For tuning: which functions were compiled, what each cost to compile, and
//! how often a call went from the interpreter into compiled code or back out,
//! since every such crossing goes through the dispatcher and is where mixed
//! mode spends its overhead. Calls from compiled code to compiled code go
//! through their stubs directly and are never seen here.

use std::fmt::Write as _;
use std::time::Duration;

use rustc_hash::FxHashMap;

use datalove_datafun_ir::IrModuleId;
use datalove_datafun_interp::FuncIdentity;

/// Statistics about JIT compilation activity.
#[derive(Clone, Debug, Default)]
pub struct JitStats {
    /// Total number of functions compiled.
    pub compiled_count: u32,
    /// Total compilation time across all functions.
    pub total_compile_time: Duration,
    /// Total generated code size in bytes.
    pub total_code_size: usize,
    /// Functions the backend declined, which are interpreted instead.
    ///
    /// Not failures: a refusal is `JitError::Unsupported`, and a failure is
    /// reported to the caller rather than counted here.
    pub refused_count: u32,
    /// Each function the jit compiled or, when calls are being counted, was
    /// called through it.
    pub functions: FxHashMap<FuncIdentity, FunctionStats>,
}

/// What the jit did with one function.
#[derive(Clone, Debug, Default)]
pub struct FunctionStats {
    /// The function's name in its module or script.
    pub name: String,
    /// How long compiling it took, if it was compiled.
    pub compile_time: Option<Duration>,
    /// The size of its code in bytes, if it was compiled.
    pub code_size: usize,
    /// Calls from the interpreter that the interpreter ran.
    ///
    /// This and the two counts after it are kept only once
    /// `JitEngine::count_calls` has been asked for.
    pub interpreted: u64,
    /// Calls from the interpreter that ran compiled code.
    pub entered: u64,
    /// Calls from compiled code that the interpreter ran.
    pub exited: u64,
}

/// Where a call the jit is offered comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallFrom {
    /// The interpreter, through the dispatcher.
    Interpreter,
    /// Compiled code, through `__jit_dispatch_call`.
    Native,
}

impl JitStats {
    /// The entry for `key`, made with `name` if there is none yet.
    pub(crate) fn function(&mut self, key: FuncIdentity, name: &str) -> &mut FunctionStats {
        self.functions.entry(key).or_insert_with(|| FunctionStats {
            name: name.to_owned(),
            ..FunctionStats::default()
        })
    }

    /// Count a call that reached the jit, by where it came from and whether
    /// compiled code ran it.
    ///
    /// `weight` is how many calls it stands for; see `JitEngine::record_call`.
    pub(crate) fn count_call(&mut self, key: FuncIdentity, name: &str, from: CallFrom, native: bool, weight: u32) {
        let f = self.function(key, name);
        let weight = weight as u64;
        match (from, native) {
            (CallFrom::Interpreter, false) => f.interpreted += weight,
            (CallFrom::Interpreter, true) => f.entered += weight,
            (CallFrom::Native, false) => f.exited += weight,
            // The compiling call of a function first called from compiled
            // code; after it the caller's stub calls the code directly.
            (CallFrom::Native, true) => {}
        }
    }

    /// A report for a person: totals, then the `top` functions with the most
    /// calls through the jit, then the `top` that took longest to compile.
    ///
    /// `module_path` names the module an `IrModuleId` is, where it is known.
    pub fn report(&self, top: usize, module_path: &dyn Fn(IrModuleId) -> Option<String>) -> String {
        let name = |key: &FuncIdentity, f: &FunctionStats| match key {
            FuncIdentity::Module { module, .. } => match module_path(*module) {
                Some(path) => format!("{path}.{}", f.name),
                None => format!("<module {}>.{}", module.0, f.name),
            },
            FuncIdentity::Unit { unit, .. } => format!("<unit {unit}>.{}", f.name),
        };

        let (interpreted, entered, exited) = self.functions.values()
            .fold((0, 0, 0), |(i, n, x), f| (i + f.interpreted, n + f.entered, x + f.exited));

        let mut out = String::new();
        let _ = writeln!(out, "jit: compiled {} functions ({} refused), {:.1} ms codegen, {} bytes",
            self.compiled_count, self.refused_count,
            self.total_compile_time.as_secs_f64() * 1e3, self.total_code_size);
        let _ = writeln!(out, "jit: calls through the dispatcher: {interpreted} interpreted, \
            {entered} interpreter -> native, {exited} native -> interpreter");

        let mut by_calls: Vec<_> = self.functions.iter()
            .filter(|(_, f)| f.interpreted + f.entered + f.exited > 0)
            .collect();
        by_calls.sort_by_key(|(key, f)| (std::cmp::Reverse(f.interpreted + f.entered + f.exited), name(key, f)));
        if !by_calls.is_empty() {
            let _ = writeln!(out, "\njit: most calls through the dispatcher");
            let _ = writeln!(out, "{:>12} {:>12} {:>12} {:>9}  function",
                "interpreted", "interp->jit", "jit->interp", "codegen");
            for (key, f) in by_calls.iter().take(top) {
                let codegen = match f.compile_time {
                    Some(t) => format!("{:.2}ms", t.as_secs_f64() * 1e3),
                    None => "-".to_owned(),
                };
                let _ = writeln!(out, "{:>12} {:>12} {:>12} {:>9}  {}",
                    f.interpreted, f.entered, f.exited, codegen, name(key, f));
            }
        }

        let mut by_codegen: Vec<_> = self.functions.iter()
            .filter_map(|(key, f)| f.compile_time.map(|t| (t, key, f)))
            .collect();
        by_codegen.sort_by_key(|(t, key, f)| (std::cmp::Reverse(*t), name(key, f)));
        if !by_codegen.is_empty() {
            let _ = writeln!(out, "\njit: longest to compile");
            let _ = writeln!(out, "{:>9} {:>8}  function", "codegen", "bytes");
            for (t, key, f) in by_codegen.iter().take(top) {
                let _ = writeln!(out, "{:>9} {:>8}  {}",
                    format!("{:.2}ms", t.as_secs_f64() * 1e3), f.code_size, name(key, f));
            }
        }
        out
    }
}
