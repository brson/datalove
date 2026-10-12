//! Calling a function body: the entry points, the frame a call runs in, its
//! arguments resolved into the callee's frame, and offering it to the
//! dispatcher first. The bytecode's planned calls are in `bytecode::call`.

use super::*;

impl IrInterpreter {
    /// Execute a function with arguments using a script environment.
    ///
    /// This allows the function to call other functions registered in the environment.
    pub fn call_with_env(
        &mut self,
        func: &IrCodeUnit,
        args: Vec<Value>,
        ret_dest: Destination,
        env: &ScriptEnvironment,
    ) -> Result<(), InterpError> {
        // Create an empty context (module functions are resolved via registry, not local context).
        let ctx = ExecutionContext::new(env.registry.unit_count(), &[]);
        // Use the environment's registry but create fresh frames (function execution
        // doesn't persist frames like script units do).
        let mut frames = FrameStore::new();
        self.call_in_context(func, None, args, ret_dest, &ctx, &env.registry, &mut frames)
    }

    /// Execute a function with arguments in a context with available functions.
    ///
    /// `code_ref` identifies the function being executed, to key its layout on.
    pub fn call_in_context(
        &mut self,
        func: &IrCodeUnit,
        code_ref: Option<CodeRef>,
        args: Vec<Value>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        self.call_in_context_with_shapes(
            func, code_ref, args, Vec::new(), ret_dest, ctx, registry, frames)
    }

    /// Call, handing the callee a descriptor for each shape it declared.
    ///
    /// Every other value here carries its own descriptor, so this is the only
    /// thing passed beside the arguments: a collection the callee builds has no
    /// value to read one off.
    #[allow(clippy::too_many_arguments)]
    pub fn call_in_context_with_shapes(
        &mut self,
        func: &IrCodeUnit,
        code_ref: Option<CodeRef>,
        args: Vec<Value>,
        shape_descriptors: Vec<*const rtdt::TyDesc>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        // The layout is the same at every call to the same body, so it is
        // remembered rather than recomputed. A caller with no reference to name
        // the callee by -- the jit's trampoline, and compile-time evaluation --
        // has nothing to key on and pays for one.
        let layout = match &code_ref {
            Some(code_ref) => {
                let key = dispatch::FuncIdentity::of(code_ref, ctx.unit());
                self.layout_cache.get_or_compute(key, func, &mut self.tydesc_table)
            }
            None => Rc::new(IrLayout::of_unit(func, &mut self.tydesc_table)),
        };

        // The trailing descriptors are the declared shapes' under erasure, and
        // the type parameters' for a generic compiled by reference: a function
        // has one or the other, which is what compiled code passes last.
        let by_reference = layout.type_param_count > 0;
        let mut frame = self.frame_stack.push(layout)?;
        for (i, arg) in args.into_iter().enumerate() {
            frame.set_param(i, arg);
        }
        for (i, tydesc) in shape_descriptors.into_iter().enumerate() {
            if by_reference {
                frame.set_type_param(i, tydesc);
            } else {
                frame.set_shape_descriptor(i, tydesc);
            }
        }
        frame.enter();
        self.enter_dynamic(&frame);
        let result = self.run_frame(func, &mut frame, ret_dest, ctx, registry, frames, code_ref.as_ref());
        self.leave_dynamic(&frame);
        self.frame_stack.pop(frame);
        self.release_retired_call_caches();
        result
    }

    /// Forget everything worked out about the bodies run so far: their
    /// layouts, their bytecode, and every call site's idea of its callee.
    ///
    /// For whoever replaces bodies, which happens between units: re-running a
    /// unit, truncating units, swapping in a new module compilation. A new body
    /// is a new allocation, which can land where a replaced one was freed, and
    /// all of that is kept by function identity or by body address, neither of
    /// which tells the two apart. Kept, an edited function ran as it was.
    pub fn forget_compiled_bodies(&mut self) {
        debug_assert!(self.frame_stack.is_empty(), "bodies replaced while frames run on them");
        self.code_epoch += 1;
        self.layout_cache = LayoutCache::new();
        if let Some(d) = self.call_dispatcher.get_mut().as_mut() {
            d.bodies_replaced();
        }
    }

    /// Free everything run so far allocated, after an error: the runtime
    /// discards all its memory at once, and everything that pointed into it
    /// is forgotten.
    ///
    /// An error unwinds frames without destroying what they own -- release
    /// frames do not track it, and compiled frames return without looking --
    /// and datalove has no side effects, so nothing is owed an orderly
    /// teardown: freeing everything is both simplest and complete. What
    /// survives an error is therefore nothing the runtime allocated: the
    /// caller's earlier units' values go too (see `run_script_unit`).
    ///
    /// Only where no frame is running, since every frame's values go.
    pub(crate) fn discard_runtime_state(&mut self) {
        debug_assert!(self.frame_stack.is_empty(), "discarding while frames run");
        // Built on the heap being discarded, so forgotten rather than
        // destroyed, and built again when next read.
        self.static_pool = Default::default();
        // Which the bytecode and the JIT's code hold the pool's addresses in.
        self.forget_compiled_bodies();
        self.retired_call_caches.clear();
        unsafe { datalove_rt::c::dtlv_rti_reset(self.runtime.handle()) };
    }

    /// Forget what was worked out about the functions of script unit `unit`,
    /// whose index is about to name another unit's.
    ///
    /// Their layouts go, and the bytecode kept with them, and every call
    /// site's idea of its callee is checked again; a module's functions,
    /// which have not changed, are lowered again only if a call site asks.
    pub fn forget_unit_bodies(&mut self, unit: u32) {
        self.code_epoch += 1;
        self.layout_cache.forget_unit(unit);
        if let Some(d) = self.call_dispatcher.get_mut().as_mut() {
            d.unit_bodies_replaced(unit);
        }
    }

    /// Let go of the call-site caches the bytecode replaced, if no frame is
    /// left that could be borrowing a layout or a body from one.
    pub(super) fn release_retired_call_caches(&mut self) {
        if self.frame_stack.is_empty() {
            self.retired_call_caches.clear();
        }
    }

    /// Run a function body in a frame that `Frame::enter` has made ready.
    #[allow(clippy::too_many_arguments)]
    fn run_frame(
        &mut self,
        func: &IrCodeUnit,
        frame: &mut Frame,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        code_ref: Option<&CodeRef>,
    ) -> Result<(), InterpError> {
        // The IR walker recurses on the native stack, a call at a time, and
        // the bytecode does where it runs a call on the walker.
        let here = 0u8;
        if (&here as *const u8 as usize) < unsafe { datalove_rt::c::dtlv_rti_native_stack_limit(self.runtime.handle()) } {
            return Err(InterpError::StackOverflow);
        }
        if matches!(self.engine, Engine::Bytecode | Engine::PlainBytecode) {
            let bc = self.bytecode_for(frame.layout(), func);
            frame.stop_keeping_liveness();
            return self.run_bytecode(&bc, func, frame, ret_dest, ctx, registry, frames, code_ref);
        }
        // Functions write their result to ret_dest, and always complete
        // normally.
        self.execute_blocks(&func.blocks, &UnitTypes::of(func), frame, ret_dest, None, ctx, registry, frames)
            .map(|_| ())
    }

    /// The bytecode for `func`, lowered against `layout` the first time.
    pub(super) fn bytecode_for(&mut self, layout: &IrLayout, func: &IrCodeUnit) -> Rc<bytecode::BcFunction> {
        let address = func as *const IrCodeUnit as usize;
        if let Some((lowered_from, bc)) = layout.bytecode.get() && *lowered_from == address {
            return Rc::clone(bc);
        }
        let (bc, escapes) = bytecode::lower(func, layout, self.engine == Engine::Bytecode);
        self.bc_stats.record(&bc, escapes);
        if std::env::var_os("DATALOVE_BC_DUMP").is_some() {
            eprintln!("{}:\n{}", func.name, bc.dump(func));
        }
        let bc = Rc::new(bc);
        // A layout already holding another body's bytecode keeps it; this one
        // is lowered again next time, which only a replaced body pays for.
        let _ = layout.bytecode.set((address, Rc::clone(&bc)));
        bc
    }

    /// Whether an `out` argument's destination currently holds a value to free.
    ///
    /// A `ValueRef` is a reference into somewhere else, which the referent's own
    /// bookkeeping covers and this frame cannot see; those are left as they were.
    fn out_dest_holds_value(op: &Operand, frame: &Frame, frames: &FrameStore) -> bool {
        match op {
            Operand::Value(id) => frame.value_is_live(*id),
            Operand::Slot(id) => frame.slot_is_live(*id),
            Operand::Param(id) => frame.param_is_live(*id),
            Operand::ExternalValue { unit, value } => {
                frames.external_value(*unit, *value).is_some()
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.is_external_slot_initialized(*unit, *slot)
            }
            Operand::ValueRef(_) => true,
        }
    }

    /// Note that an `out` destination has been emptied, so that nothing frees
    /// it twice before the callee writes it.
    fn mark_out_dest_cleared(op: &Operand, frame: &mut Frame) {
        match op {
            Operand::Value(id) => frame.mark_value_dropped(*id),
            Operand::Slot(id) => frame.mark_slot_dropped(*id),
            Operand::Param(id) => frame.mark_param_dropped(*id),
            Operand::ValueRef(_) | Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
        }
    }

    /// Resolve one argument for a parameter passed in `mode`.
    ///
    /// An `out` argument is the place the callee will write, emptied first if
    /// it holds anything. Any other is the value as the caller has it, read
    /// through a wrapper when it is borrowed.
    fn resolve_arg(
        &self,
        mode: ParamMode,
        op: &Operand,
        frame: &mut Frame,
        frames: &FrameStore,
        scratch: &mut BorrowScratch,
    ) -> Value {
        if mode == ParamMode::Out {
            // Out param: the callee writes over whatever is there, so what
            // is there has to be destroyed first -- unless nothing is. A
            // destination that has never been written holds no value to
            // free, and one already passed on as an out parameter was
            // cleared by whoever called this function.
            //
            // This used to ask the memory rather than the frame, and rested
            // on the frame being zeroed so that destroying a place that had
            // never been written read a null pointer and did nothing. That
            // made "uninitialized" and "empty" indistinguishable, which is
            // the invariant the compiled backends carry a tracking byte for
            // rather than assume.
            let val = self.get_operand_dest(op, frame);
            if Self::out_dest_holds_value(op, frame, frames) {
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        val.ptr,
                        val.tydesc,
                    );
                }
                Self::mark_out_dest_cleared(op, frame);
            }
            val
        } else {
            let mut val = self.read_operand(op, frame, frames);
            // A container of a type parameter travels wrapped once it is
            // owned, and a borrowed parameter wants the container itself
            // with a descriptor beside it. Both are inside the wrapper.
            if matches!(mode, ParamMode::Ref | ParamMode::Mut) {
                val = Self::borrow_through_wrapper(val, scratch);
            }
            val
        }
    }

    /// The descriptor a `DescriptorRef` names.
    ///
    /// A static one is built from its type the way any type's is; a forwarded
    /// one is what this function was itself handed. Nothing is put together
    /// here: the call site named a whole type.
    pub(super) fn resolve_shape_ref(
        &mut self,
        r: &datalove_datafun_ir::DescriptorRef,
        frame: &Frame,
    ) -> *const rtdt::TyDesc {
        match r {
            datalove_datafun_ir::DescriptorRef::Static(ty) => {
                self.tydesc_table.get_or_create(ty)
            }
            datalove_datafun_ir::DescriptorRef::Own(index) => frame
                .shape_descriptor(*index)
                .expect("a forwarded shape is one this function declared"),
        }
    }

    /// Read through a wrapper, where one is what arrived.
    ///
    /// Only a wrapped value has anything to read through, and only a borrowed
    /// parameter asks: an owned one takes the wrapper as it stands, since
    /// owning it means dropping it and the wrapper is what knows how.
    ///
    /// A wrapper holding something on the heap lends the address it has. One
    /// holding a narrow scalar in its own words has no address to lend, so the
    /// value is unpacked into `scratch`, which the caller keeps alive for as
    /// long as the borrow.
    pub(super) fn borrow_through_wrapper(val: Value, scratch: &mut BorrowScratch) -> Value {
        if unsafe { (*val.tydesc).type_tag } != rtdt::TyTag::Data {
            return val;
        }
        // Only a value the wrapper keeps in its own words is unpacked, so only
        // that takes a box; one on the heap is lent where it is. This runs on
        // every call that passes a wrapped container on.
        let mut inner_ptr: *const u8 = std::ptr::null();
        let mut inner_tydesc: *const rtdt::TyDesc = std::ptr::null();
        let held_inline = unsafe { (*(val.ptr as *const rtdt::Data)).tag() }
            != rtdt::anypack::Tag::TwoPointers;
        let status = if held_inline {
            scratch.push(Box::new(0u64));
            let slot = scratch.last_mut().expect("just pushed").as_mut() as *mut u64 as *mut u8;
            unsafe { datalove_rt::c::dtlv_rti_data_borrow(val.ptr, slot, &mut inner_ptr, &mut inner_tydesc) }
        } else {
            unsafe { datalove_rt::c::dtlv_rti_data_parts(val.ptr, &mut inner_ptr, &mut inner_tydesc) }
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "borrowing through a wrapper that holds nothing to borrow");
        Value { ptr: inner_ptr as *mut u8, tydesc: inner_tydesc }
    }

    /// Try to dispatch a call via the JIT dispatcher.
    ///
    /// Returns `Some(result)` if the dispatcher handled the call,
    /// `None` if it should fall through to the interpreter.
    fn try_dispatch_call(
        &mut self,
        func: &CodeRef,
        callee: &IrCodeUnit,
        arg_vals: &[Value],
        shape_descriptors: &[*const rtdt::TyDesc],
        dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Option<Result<(), InterpError>> {
        let rt_handle = self.runtime.handle();
        let weight = std::mem::replace(&mut self.call_weight, 1);
        let result = self.with_dispatcher(ctx, registry, frames, shape_descriptors, weight,
            |dispatcher, call_ctx| dispatcher.dispatch_call(func, callee, arg_vals, dest, rt_handle, call_ctx))?;
        match result {
            dispatch::DispatchResult::Handled(result) => Some(result),
            dispatch::DispatchResult::NotHandled => None,
        }
    }

    /// Run `f` with the dispatcher and a context for it, or say there is no
    /// dispatcher.
    ///
    /// The dispatcher is taken out of the interpreter for as long as `f` runs,
    /// so that it and the interpreter can each be borrowed; compiled code it
    /// runs that calls back into the interpreter lends it a stand-in
    /// (`lend_dispatcher`). `exec_ctx` and the rest are the call's or the
    /// frame's, as `DispatchCallContext` says.
    pub(crate) fn with_dispatcher<R>(
        &mut self,
        exec_ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        shape_descriptors: &[*const rtdt::TyDesc],
        weight: u32,
        f: impl FnOnce(&mut dyn CallDispatcher, dispatch::DispatchCallContext<'_, '_>) -> R,
    ) -> Option<R> {
        let mut dispatcher = self.call_dispatcher.borrow_mut().take()?;
        let call_ctx = dispatch::DispatchCallContext {
            exec_ctx,
            registry,
            frames,
            interp: self,
            shape_descriptors,
            weight,
        };
        let result = f(&mut *dispatcher, call_ctx);
        *self.call_dispatcher.borrow_mut() = Some(dispatcher);
        Some(result)
    }

    /// The frame layout for a function, computed once per body.
    ///
    /// For a caller that is about to describe the same function's parameters and
    /// return -- the jit's trampoline does, once per call out of compiled code --
    /// so that it reads them rather than looking each type up again.
    pub fn layout_for(
        &mut self,
        func: dispatch::FuncIdentity,
        unit: &IrCodeUnit,
    ) -> Rc<IrLayout> {
        self.layout_cache.get_or_compute(func, unit, &mut self.tydesc_table)
    }

    /// Execute a call instruction.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_call(
        &mut self,
        code_ref: &CodeRef,
        args: &[Operand],
        shape_refs: &[datalove_datafun_ir::DescriptorRef],
        bindings: &[IrType],
        dest: datalove_datafun_ir::ValueId,
        frame: &mut Frame,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        let callee = ctx.get_unit(code_ref, registry);
        match &callee.context {
            datalove_datafun_ir::CodeUnitContext::Native(native_ctx) => {
                self.execute_native_call(native_ctx, args, shape_refs, bindings, dest, frame, frames)
            }
            _ => self.execute_call_site(
                code_ref, callee, args, shape_refs, bindings, dest,
                frame, ctx, registry, frames),
        }
    }

    /// The descriptor of what a call site bound a type parameter to.
    ///
    /// A concrete binding has a static descriptor. One written over the
    /// caller's own type parameters is an entry of the caller's descriptor
    /// table, filled when the caller was entered.
    pub(super) fn binding_descriptor(&mut self, binding: &IrType, frame: &Frame) -> *const rtdt::TyDesc {
        let r = self.binding_ref(binding, frame.layout());
        Self::resolve_binding(r, frame)
    }

    /// Where the descriptor of what a call site bound a type parameter to is
    /// found, worked out once for a site to keep.
    pub(crate) fn binding_ref(&mut self, binding: &IrType, caller: &IrLayout) -> crate::BindingRef {
        if !binding.is_dynamic() {
            return crate::BindingRef::Static(self.tydesc_table.get_or_create(binding));
        }
        match caller.table_types.iter().position(|t| t == binding) {
            Some(index) => crate::BindingRef::Table(index as u32),
            None => panic!("{binding} is bound at a call and not in the caller's descriptor table"),
        }
    }

    /// The descriptor a `BindingRef` names, in the caller's frame.
    #[inline]
    pub(crate) fn resolve_binding(r: crate::BindingRef, frame: &Frame) -> *const rtdt::TyDesc {
        match r {
            crate::BindingRef::Static(tydesc) => tydesc,
            crate::BindingRef::Table(index) => frame.table_entry(index as usize),
        }
    }

    /// The descriptor of `ty` with a generic's type parameters bound to
    /// `type_params`: a static one for a concrete type, and one instantiated
    /// from its template for a dynamic one.
    ///
    /// For a caller that has a generic's type parameter descriptors and not
    /// its frame, which the jit's trampoline is: compiled code passes a
    /// dynamic argument as a pointer, and the interpreter wants it described.
    pub fn describe(&mut self, ty: &IrType, type_params: &[*const rtdt::TyDesc]) -> *const rtdt::TyDesc {
        let template = self.tydesc_table.get_or_create(ty);
        if !ty.is_dynamic() {
            return template;
        }
        unsafe {
            datalove_rt::c::dtlv_rti_tydesc_instantiate(
                self.runtime.handle(), template, type_params.as_ptr(), type_params.len() as u32)
        }
    }

    /// Make a frame of a function compiled by reference ready to run, once
    /// its type parameters' descriptors are set: fill its descriptor table,
    /// and point each of its dynamic values and slots at space on the shadow
    /// stack.
    pub(super) fn enter_dynamic(&mut self, frame: &Frame) {
        let layout = frame.layout();
        if layout.table.is_empty() {
            return;
        }
        let rt = self.runtime.handle();
        let params = frame.type_params();
        let mut cache = layout.table_cache.borrow_mut();
        if cache.0 == params {
            for (i, &tydesc) in cache.1.iter().enumerate() {
                frame.set_table_entry(i, tydesc);
            }
        } else {
            cache.1.clear();
            for (i, entry) in layout.table.iter().enumerate() {
                let tydesc = match *entry {
                    crate::layout::TableEntry::Param(p) => params[p as usize],
                    crate::layout::TableEntry::Template(template) => unsafe {
                        datalove_rt::c::dtlv_rti_tydesc_instantiate(
                            rt, template, params.as_ptr(), params.len() as u32)
                    },
                };
                frame.set_table_entry(i, tydesc);
                cache.1.push(tydesc);
            }
            cache.0.clear();
            cache.0.extend_from_slice(params);
        }
        drop(cache);
        if let Some(mark) = layout.shadow_mark_offset {
            // SAFETY: the handle's header, and the frame's mark words.
            let header = rt as *mut datalove_rt::c::RtHeader;
            unsafe { *frame.shadow_mark(mark) = [(*header).shadow_top, (*header).shadow_limit] };
            for &(offset, entry) in &layout.dynamic_places {
                let tydesc = frame.table_entry(entry as usize);
                let (size, align) = unsafe { ((*tydesc).size, (*tydesc).align) };
                // Bumped here, as compiled code does, and the runtime asked
                // only when its chunk has no room.
                let ptr = unsafe {
                    let start = ((*header).shadow_top + align as usize - 1) & !(align as usize - 1);
                    if start + size as usize <= (*header).shadow_limit {
                        (*header).shadow_top = start + size as usize;
                        start as *mut u8
                    } else {
                        datalove_rt::c::dtlv_rti_shadow_alloc(rt, size, align)
                    }
                };
                frame.set_fat_pointer(offset, ptr, tydesc);
            }
        }
    }

    /// Give back the shadow stack space `enter_dynamic` took.
    pub(super) fn leave_dynamic(&mut self, frame: &Frame) {
        if let Some(mark) = frame.layout().shadow_mark_offset {
            // SAFETY: as in `enter_dynamic`.
            unsafe {
                let header = self.runtime.handle() as *mut datalove_rt::c::RtHeader;
                let [top, limit] = *frame.shadow_mark(mark);
                (*header).shadow_top = top;
                (*header).shadow_limit = limit;
            }
        }
    }

    /// Call a native function.
    ///
    /// A native has no frame, so its arguments are collected into a list, and
    /// the descriptors for its shapes go after them as they do at a call to a
    /// module function. Otherwise the arguments are handled as
    /// `execute_call_site` handles them.
    fn execute_native_call(
        &mut self,
        native_ctx: &datalove_datafun_ir::NativeContext,
        args: &[Operand],
        shape_refs: &[datalove_datafun_ir::DescriptorRef],
        bindings: &[IrType],
        dest: datalove_datafun_ir::ValueId,
        frame: &mut Frame,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        let mode = |i: usize| native_ctx.param_modes.get(i).copied().unwrap_or(ParamMode::In);

        // Held until the call returns, because a borrowed argument with no
        // address of its own points into it.
        let mut scratch: BorrowScratch = Vec::new();
        let arg_vals: Vec<Value> = args.iter().enumerate()
            .map(|(i, op)| self.resolve_arg(mode(i), op, frame, frames, &mut scratch))
            .collect();
        for (i, op) in args.iter().enumerate() {
            // A native's type parameter is a `data` in its signature, which a
            // copy argument bound to it is not, under generics by reference;
            // whether the argument is given up is its own type's to say.
            let moves = match &native_ctx.param_types[i] {
                IrType::Data if !bindings.is_empty() => !is_plain(&arg_vals[i]),
                ty => !ty.is_copy(),
            };
            if mode(i) == ParamMode::In && moves {
                Self::mark_source_dropped_local(op, frame);
            }
        }
        // Under generics by reference a native's descriptors come from what
        // the call bound its type parameters to; under erasure, from the
        // shapes worked out across the call graph.
        let supplied: Vec<*const rtdt::TyDesc> = if !bindings.is_empty() {
            native_ctx.descriptor_shapes.iter()
                .map(|shape| self.binding_descriptor(&shape.to_ir_type().substitute(bindings), frame))
                .collect()
        } else {
            shape_refs.iter()
                .map(|r| self.resolve_shape_ref(r, frame))
                .collect()
        };
        let dest_slot = frame.value_dest(dest);

        self.native_table.call(
            native_ctx.symbol(), self.runtime.handle(), &arg_vals, dest_slot, &supplied,
        )?;

        frame.mark_value_live(dest);
        for (i, op) in args.iter().enumerate() {
            if mode(i) == ParamMode::Out {
                Self::mark_out_written(op, frame);
            }
        }
        Ok(())
    }

    /// Note that a call has written an `out` argument.
    fn mark_out_written(op: &Operand, frame: &mut Frame) {
        match op {
            Operand::Slot(id) => frame.mark_slot_live(*id),
            Operand::Value(id) => frame.mark_value_live(*id),
            // This function's own `out` parameter, passed straight on.
            Operand::Param(id) => frame.mark_param_live(*id),
            Operand::ValueRef(_) | Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
        }
    }

    /// Call a function that is not native from a call instruction.
    ///
    /// The arguments are resolved straight into the callee's frame rather than
    /// into a list for the frame to copy: the frame's parameters are where they
    /// end up, and a dispatcher is offered them there. Only if it declines is
    /// the rest of the frame made ready, so a call the jit takes does not pay
    /// for it.
    #[allow(clippy::too_many_arguments)]
    fn execute_call_site(
        &mut self,
        code_ref: &CodeRef,
        callee: &IrCodeUnit,
        args: &[Operand],
        shape_refs: &[datalove_datafun_ir::DescriptorRef],
        bindings: &[IrType],
        dest: datalove_datafun_ir::ValueId,
        frame: &mut Frame,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        let identity = dispatch::FuncIdentity::of(code_ref, ctx.unit());
        let layout = self.layout_cache.get_or_compute(identity, callee, &mut self.tydesc_table);
        let mut callee_frame = self.frame_stack.push(layout)?;

        // Held until the call returns, because a borrowed argument with no
        // address of its own points into it.
        let mut scratch: BorrowScratch = Vec::new();
        for (i, op) in args.iter().enumerate() {
            let mode = callee_frame.layout().param_modes[i];
            let val = self.resolve_arg(mode, op, frame, frames, &mut scratch);
            callee_frame.set_param(i, val);
        }
        // After every argument is read, in case one place is passed twice.
        //
        // A type parameter is never a copy type in the callee, which owns
        // whatever it is handed, but the argument may be: a copy value handed
        // to a `T` is copied, not given up. So for a dynamic parameter the
        // argument's own descriptor says.
        for (i, op) in args.iter().enumerate() {
            let layout = callee_frame.layout();
            let moves = if layout.param_tydescs[i].is_null() {
                layout.param_modes[i] == ParamMode::In && !is_plain(&callee_frame.params()[i])
            } else {
                layout.param_moves[i]
            };
            if moves {
                Self::mark_source_dropped_local(op, frame);
            }
        }
        for (i, r) in shape_refs.iter().enumerate() {
            let tydesc = self.resolve_shape_ref(r, frame);
            callee_frame.set_shape_descriptor(i, tydesc);
        }
        let by_reference = callee_frame.layout().type_param_count > 0;
        for (i, binding) in bindings.iter().enumerate() {
            let tydesc = self.binding_descriptor(binding, frame);
            callee_frame.set_type_param(i, tydesc);
        }
        let dest_slot = frame.value_dest(dest);

        // The trailing descriptors a compiled callee takes: its shapes', or
        // its type parameters'.
        let trailing = if by_reference {
            callee_frame.type_params()
        } else {
            callee_frame.shape_descriptors()
        };
        let dispatched = self.try_dispatch_call(
            code_ref, callee, callee_frame.params(), trailing,
            dest_slot, ctx, registry, frames);
        let result = match dispatched {
            Some(result) => result,
            None => {
                callee_frame.enter();
                self.enter_dynamic(&callee_frame);
                let callee_ctx = ctx.for_callee(code_ref, registry);
                let result = self.run_frame(
                    callee, &mut callee_frame, dest_slot, &callee_ctx, registry, frames,
                    Some(code_ref));
                self.leave_dynamic(&callee_frame);
                result
            }
        };

        if result.is_ok() {
            frame.mark_value_live(dest);
            for (i, op) in args.iter().enumerate() {
                if callee_frame.layout().param_modes[i] == ParamMode::Out {
                    Self::mark_out_written(op, frame);
                }
            }
        }
        self.frame_stack.pop(callee_frame);
        result
    }
}

/// Whether a value owns nothing, so that handing it over copies it rather
/// than giving it up.
fn is_plain(value: &Value) -> bool {
    // SAFETY: a value's descriptor.
    unsafe { (*value.tydesc).flags & rtdt::TyDesc::PLAIN != 0 }
}
