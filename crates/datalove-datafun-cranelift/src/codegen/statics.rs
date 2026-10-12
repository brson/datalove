//! Static consts: where a `StaticRef` finds one, and the passes that build
//! and destroy them in an object file.

use super::*;

/// Where a const a `StaticRef` names lives.
///
/// Under the JIT the interpreter has already built it, and this is its
/// address. In an object file it is a data object the program fills in before
/// the script runs; see [`FunctionCompiler::compile_static_init`].
#[derive(Clone, Copy, Debug)]
pub enum StaticConstLoc {
    Address(usize),
    Data(cranelift_module::DataId),
}

/// Where each static const lives, keyed by the address of the `Arc` a
/// `StaticRef` holds its value in.
pub type StaticConsts = HashMap<usize, StaticConstLoc>;

/// The key a `StaticRef`'s value is found under in [`StaticConsts`].
pub fn static_const_key(value: &std::sync::Arc<ConstValue>) -> usize {
    std::sync::Arc::as_ptr(value) as usize
}

/// A function unit with no parameters and no body, returning nothing.
///
/// What [`FunctionCompiler::compile_static_init`] and
/// [`FunctionCompiler::compile_static_fini`] are compiled over: they need a
/// compiler's runtime and descriptors, and no IR of their own.
pub fn empty_function_unit(name: &str) -> IrCodeUnit {
    IrCodeUnit {
        id: datalove_datafun_ir::CodeUnitId(0),
        name: name.to_string(),
        blocks: Vec::new(),
        value_count: 0,
        slot_count: 0,
        value_types: Vec::new(),
        slot_types: Vec::new(),
        tracked_slots: Vec::new(),
        const_values: Vec::new(),
        symbols: Default::default(),
        context: datalove_datafun_ir::CodeUnitContext::Function(
            datalove_datafun_ir::FunctionContext {
                params: Vec::new(),
                param_modes: Vec::new(),
                param_types: Vec::new(),
                return_type: IrType::Unit,
                tracked_params: Vec::new(),
                type_param_count: 0,
                descriptor_table: Vec::new(),
            },
        ),
        nested_units: Vec::new(),
    }
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Say where the consts `StaticRef` names live.
    ///
    /// A body naming one this does not list is refused as unsupported.
    pub fn set_static_consts(&mut self, static_consts: StaticConsts) {
        self.static_consts = static_consts;
    }

    /// Compile a function, taking only the runtime handle, that builds each
    /// static const into its data object.
    ///
    /// The compiler is made over [`empty_function_unit`]. The program calls
    /// this before the script runs.
    pub fn compile_static_init(
        mut self,
        func_id: FuncId,
        statics: &[(cranelift_module::DataId, IrType, std::sync::Arc<ConstValue>)],
    ) -> Result<(), CraneliftError> {
        self.compile_static_pass(func_id, |this, builder, addr, ty, value| {
            this.write_const_value_to_addr(builder, addr, ty, value)
        }, statics)
    }

    /// Compile a function, taking only the runtime handle, that destroys each
    /// static const [`Self::compile_static_init`] built.
    ///
    /// The program calls this before the runtime shuts down, which is when the
    /// runtime looks for leaks.
    pub fn compile_static_fini(
        mut self,
        func_id: FuncId,
        statics: &[(cranelift_module::DataId, IrType, std::sync::Arc<ConstValue>)],
    ) -> Result<(), CraneliftError> {
        self.compile_static_pass(func_id, |this, builder, addr, ty, _value| {
            let runtime = this.runtime;
            let tydesc_id = this.tydesc(ty)?;
            let tydesc_gv = this.module.declare_data_in_func(tydesc_id, builder.func);
            let tydesc = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);
            let destroy = this.module.declare_func_in_func(runtime.destroy_local, builder.func);
            let rt = this.rt_handle_param.expect("set at entry");
            builder.ins().call(destroy, &[rt, addr, tydesc]);
            Ok(())
        }, statics)
    }

    /// Compile a function over each static const's address in turn.
    fn compile_static_pass(
        &mut self,
        func_id: FuncId,
        mut each: impl FnMut(&mut Self, &mut FunctionBuilder, cl_ir::Value, &IrType, &ConstValue)
            -> Result<(), CraneliftError>,
        statics: &[(cranelift_module::DataId, IrType, std::sync::Arc<ConstValue>)],
    ) -> Result<(), CraneliftError> {
        let sig = self.build_signature();
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        self.rt_handle_param = Some(builder.block_params(entry)[0]);

        for (data_id, ty, value) in statics {
            let gv = self.module.declare_data_in_func(*data_id, builder.func);
            let addr = builder.ins().symbol_value(PTR_TYPE, gv);
            each(self, &mut builder, addr, ty, value)?;
        }
        builder.ins().return_(&[]);
        builder.finalize(self.isa.frontend_config());

        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CraneliftError::Codegen(format!("define function: {}", e)))?;
        Ok(())
    }
}
