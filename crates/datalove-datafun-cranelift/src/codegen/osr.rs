//! Entering a function at a loop header from an interpreter frame already
//! running it (on-stack replacement): what is compiled, how the frame is read,
//! and the values the loop reads that the code reached also makes.

use super::*;

/// The blocks reachable from `from`, itself included.
pub(super) fn reachable_from(func: &IrCodeUnit, from: BlockId) -> std::collections::HashSet<BlockId> {
    let mut seen = std::collections::HashSet::new();
    let mut work = vec![from];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let block = func.blocks.iter().find(|b| b.id == id).expect("a block of the function");
        work.extend(block.terminator.successors());
    }
    seen
}

/// The values an OSR entry at `header` reads before the code it reaches has
/// defined them, which it also defines: what an enclosing loop's body makes
/// and the inner loop reads, among them.
///
/// Such a value comes from two places -- the interpreter's frame, the first
/// time round, and its definition, every time after -- so it is compiled as a
/// variable, given its frame value on entry and its new one where it is made,
/// and Cranelift puts the merge where the two meet.
///
/// In order, so that the variables, and the code, come out the same each time.
pub(super) fn carried_into(
    func: &IrCodeUnit,
    reachable: &std::collections::HashSet<BlockId>,
    header: BlockId,
) -> std::collections::BTreeSet<ValueId> {
    use std::collections::HashSet;
    let blocks: Vec<&IrBlock> = func.blocks.iter().filter(|b| reachable.contains(&b.id)).collect();
    let defines = |block: &IrBlock| -> HashSet<ValueId> {
        block.params.iter().copied().chain(block.instructions.iter().flat_map(Instruction::dests)).collect()
    };
    let defined: HashMap<BlockId, HashSet<ValueId>> = blocks.iter().map(|b| (b.id, defines(b))).collect();

    // What each block reads that it does not define, then live-in sets by the
    // usual backward iteration.
    let mut uses: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
    for block in &blocks {
        let mut used = HashSet::new();
        let mut add = |op: &Operand| {
            if let Operand::Value(v) | Operand::ValueRef(v) = op {
                used.insert(*v);
            }
        };
        block.instructions.iter().for_each(|i| i.for_each_operand(&mut add));
        block.terminator.for_each_operand(&mut add);
        used.retain(|v| !defined[&block.id].contains(v));
        uses.insert(block.id, used);
    }
    let mut live_in = uses.clone();
    loop {
        let mut changed = false;
        for block in blocks.iter().rev() {
            let mut live: HashSet<ValueId> = block.terminator.successors().into_iter()
                .filter_map(|s| live_in.get(&s))
                .flatten()
                .copied()
                .collect();
            live.retain(|v| !defined[&block.id].contains(v));
            live.extend(uses[&block.id].iter().copied());
            let entry = live_in.get_mut(&block.id).expect("every reached block");
            if live.len() != entry.len() {
                *entry = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let all: HashSet<ValueId> = defined.into_values().flatten().collect();
    live_in.remove(&header).expect("the header is reached").into_iter().filter(|v| all.contains(v)).collect()
}

/// An entry into a function at one of its loop headers rather than at its
/// start, from the interpreter's frame for a call already running it
/// (on-stack replacement).
///
/// The code takes the runtime handle, the `sret` pointer if the function has
/// one, and the frame, and carries on in the frame in place: values, slots
/// and tracking bytes are where `FrameLayout` puts them in both. The rest is
/// where the interpreter keeps it, which this says.
pub struct OsrSpec {
    /// The block entered, a loop header.
    pub header: BlockId,
    /// Where each parameter's `Value` is: its pointer, then its descriptor.
    pub param_offsets: Vec<u32>,
    /// Where the descriptor table is, one word an entry, filled when the
    /// interpreter entered the function.
    pub table_offset: u32,
}

/// What compiling an OSR entry keeps; see `OsrSpec`.
pub(super) struct OsrState {
    /// The blocks reachable from the header, which are all that is compiled.
    pub(super) reachable: std::collections::HashSet<BlockId>,
    /// The interpreter's frame.
    pub(super) frame: cl_ir::Value,
    /// The entry block's jump to the header, before which a value defined
    /// before the loop is read in, the first time something asks for it.
    pub(super) entry_jump: cl_ir::Inst,
    /// Those values, read in.
    pub(super) materialized: std::cell::RefCell<HashMap<ValueId, cl_ir::Value>>,
    /// The constants, a scalar one of which is made again rather than read:
    /// the bytecode folds a constant into what reads it and may never write
    /// it to the frame.
    pub(super) consts: HashMap<ValueId, ConstValue>,
    /// The values read from the frame on entry and made again in the code
    /// reached, as variables; see `carried_into`.
    pub(super) carried: HashMap<ValueId, Variable>,
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile an entry into the function at a loop header, from an
    /// interpreter frame; see `OsrSpec`.
    ///
    /// Only what can be reached from the header is compiled. A value the
    /// loop reads that the code reached also makes -- an enclosing loop's, for
    /// an inner loop's header -- is a variable; see `carried_into`.
    pub fn compile_osr_as(mut self, symbol: &str, spec: OsrSpec) -> Result<Defined, CraneliftError> {
        let mut sig = cl_ir::Signature::new(self.isa.default_call_conv());
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        if uses_sret(&self.func_ctx.return_type) {
            sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        }
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        let func_id = self.module
            .declare_function(symbol, Linkage::Export, &sig)
            .map_err(|e| CraneliftError::Module(format!("declare function: {}", e)))?;
        self.osr_spec = Some(spec);
        self.compile_body(func_id, sig)
    }

    /// In an OSR entry, the value the interpreter's frame holds for `vid`,
    /// read in once, before the entry's jump to the header.
    pub(super) fn materialize(&self, builder: &mut FunctionBuilder, vid: ValueId) -> Result<cl_ir::Value, CraneliftError> {
        let osr = self.osr.as_ref().expect("an OSR entry");
        if let Some(v) = osr.materialized.borrow().get(&vid) {
            return Ok(*v);
        }
        use cranelift_codegen::cursor::{Cursor, FuncCursor};
        let ty = &self.func.value_types[vid.0 as usize];
        let offset = self.layout.value_offset(vid.0) as i32;
        let mut pos = FuncCursor::new(builder.func).at_inst(osr.entry_jump);
        let remade = osr.consts.get(&vid).and_then(|c| constants::scalar_const(pos.ins(), c));
        // A dynamic value is where its pointer in the frame says.
        if let Some(&ptr) = self.dyn_storage.get(&vid) {
            osr.materialized.borrow_mut().insert(vid, ptr);
            return Ok(ptr);
        }
        let v = match (remade, types::ir_type_to_cranelift(ty)) {
            (Some(v), _) => v,
            (None, CraneliftRepr::Scalar(cl_ty)) => pos.ins().load(cl_ty, MemFlagsData::trusted(), osr.frame, offset),
            (None, CraneliftRepr::Aggregate(_)) => pos.ins().iadd_imm_s(osr.frame, offset as i64),
        };
        osr.materialized.borrow_mut().insert(vid, v);
        Ok(v)
    }

    /// Set up an OSR entry's own entry block: the parameters and descriptors
    /// from where the interpreter keeps them, and a jump to the header with
    /// its parameters as the frame holds them.
    pub(super) fn enter_osr(&mut self, builder: &mut FunctionBuilder, reachable: std::collections::HashSet<BlockId>) -> Result<(), CraneliftError> {
        let spec = self.osr_spec.as_ref().expect("an OSR entry");
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let params = builder.block_params(entry).to_vec();
        self.rt_handle_param = Some(params[0]);
        let has_sret = uses_sret(&self.func_ctx.return_type);
        if has_sret {
            self.sret_param = Some(params[1]);
        }
        let frame = params[if has_sret { 2 } else { 1 }];
        self.frame_slot = Some(FrameBase::Ptr(frame));

        let flags = MemFlagsData::trusted();
        let word = types::PTR_SIZE as i32;
        for (i, &offset) in spec.param_offsets.iter().enumerate() {
            let ptr = builder.ins().load(PTR_TYPE, flags, frame, offset as i32);
            self.param_values.insert(ParamId(i as u32), ptr);
        }

        // A generic's table, and where its dynamic values and slots are,
        // as the interpreter set them up on entry. The shadow stack space is
        // the interpreter's, which gives it back.
        self.table_values = (0..self.func_ctx.descriptor_table.len())
            .map(|k| builder.ins().load(PTR_TYPE, flags, frame, spec.table_offset as i32 + k as i32 * word))
            .collect();
        for (i, ty) in self.func.value_types.iter().enumerate() {
            if dynamic::is_dynamic_value(ty) {
                let offset = self.layout.value_offset(i as u32) as i32;
                let ptr = builder.ins().load(PTR_TYPE, flags, frame, offset);
                self.dyn_storage.insert(ValueId(i as u32), ptr);
            }
        }
        for (i, ty) in self.func.slot_types.iter().enumerate() {
            if dynamic::is_dynamic_value(ty) {
                let offset = self.layout.slot_offset(i as u32) as i32;
                let ptr = builder.ins().load(PTR_TYPE, flags, frame, offset);
                self.dyn_slot_storage.insert(SlotId(i as u32), ptr);
            }
        }

        // The header's parameters as an edge into it would pass them: a
        // scalar itself, an aggregate by its address.
        let header = self.func.blocks.iter().find(|b| b.id == spec.header)
            .expect("the header is a block of the function");
        let args: Vec<cranelift_codegen::ir::BlockArg> = header.params.iter().map(|&v| {
            let offset = self.layout.value_offset(v.0) as i32;
            if let Some(&ptr) = self.dyn_storage.get(&v) {
                return cranelift_codegen::ir::BlockArg::from(ptr);
            }
            let arg = match types::ir_type_to_cranelift(&self.func.value_types[v.0 as usize]) {
                CraneliftRepr::Scalar(cl_ty) => builder.ins().load(cl_ty, flags, frame, offset),
                CraneliftRepr::Aggregate(_) => builder.ins().iadd_imm_s(frame, offset as i64),
            };
            cranelift_codegen::ir::BlockArg::from(arg)
        }).collect();
        let entry_jump = builder.ins().jump(self.blocks[&spec.header], &args);

        let consts = self.func.blocks.iter()
            .flat_map(|b| &b.instructions)
            .filter_map(|instr| match instr {
                Instruction::Const { dest, value } => Some((*dest, value.clone())),
                _ => None,
            })
            .collect();
        self.osr = Some(OsrState {
            reachable,
            frame,
            entry_jump,
            materialized: std::cell::RefCell::new(HashMap::new()),
            consts,
            carried: HashMap::new(),
        });

        // Each carried value's first definition is what the frame holds.
        let header = spec.header;
        let reachable = &self.osr.as_ref().expect("just made").reachable;
        let carried = carried_into(self.func, reachable, header);
        for vid in carried {
            let cl_ty = match types::ir_type_to_cranelift(&self.func.value_types[vid.0 as usize]) {
                CraneliftRepr::Scalar(cl_ty) => cl_ty,
                CraneliftRepr::Aggregate(_) => PTR_TYPE,
            };
            let var = builder.declare_var(cl_ty);
            let from_frame = self.materialize(builder, vid)?;
            builder.def_var(var, from_frame);
            self.osr.as_mut().expect("just made").carried.insert(vid, var);
        }
        Ok(())
    }
}
