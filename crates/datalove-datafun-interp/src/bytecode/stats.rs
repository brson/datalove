//! What lowering did, for `DATALOVE_BC_STATS`.

use super::*;

impl BcStats {
    pub(crate) fn record(&mut self, bc: &BcFunction, escapes: Vec<String>) {
        self.bodies += 1;
        self.ops += bc.ops.len() as u32;
        self.escapes += escapes.len() as u32;
        self.escaped_kinds.extend(escapes);
        for op in &bc.ops {
            let text = format!("{op:?}");
            self.op_kinds.insert(text.split([' ', '{', '(']).next().unwrap_or("").to_string());
        }
    }
}

impl IrInterpreter {
    /// Print what the bytecode lowering did, for `DATALOVE_BC_STATS`.
    pub(crate) fn report_bc_stats(&self) {
        if matches!(self.engine, crate::Engine::Bytecode | crate::Engine::PlainBytecode)
            && std::env::var_os("DATALOVE_BC_STATS").is_some()
        {
            let s = &self.bc_stats;
            if s.bodies == 0 {
                return;
            }
            eprintln!("bytecode: {} bodies, {} ops, {} on the IR walker", s.bodies, s.ops, s.escapes);
            let mut executed: Vec<_> = s.executed.iter().collect();
            executed.sort_by(|a, b| b.1.cmp(a.1));
            for (what, n) in executed.iter().take(15) {
                eprintln!("  {n:>12}  {what}");
            }
        }
    }
}
