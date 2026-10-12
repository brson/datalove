//! Calls out of the bytecode: frames switched to in place, planned calls into
//! natives and compiled code, the general path, and a loop handed to OSR.

use super::*;

impl IrInterpreter {
    /// Save the running body's registers and carry on in `callee`, whose frame
    /// is pushed and entered; `pc` is where the caller goes on from. Returns
    /// where the callee starts.
    #[inline(always)]
    pub(super) fn switch_to<'r>(&mut self, regs: &mut Regs<'r>, callee: Callee<'r>, pc: usize) -> usize {
        push_in_place(&mut regs.stack, Activation {
            bc: regs.bc,
            bc_keep: std::mem::replace(&mut regs.bc_keep, callee.keep),
            code_ref: regs.code_ref,
            ctx: callee.ctx.map(|ctx| std::mem::replace(&mut regs.ctx, ctx)),
            base: regs.frame.base_ptr(),
            layout: regs.frame.layout_ptr(),
            ret_dest: regs.ret_dest,
            pc,
        });
        regs.bc = callee.bc;
        regs.func = callee.func;
        regs.code_ref = Some(callee.code_ref);
        regs.frame = callee.frame;
        regs.ret_dest = callee.dest;
        // SAFETY: kept alive by `bc_keep` or by the plan the call was made by.
        unsafe { (*regs.bc).entry as usize }
    }

    /// Pop the running frame and go back to its caller, or say there is none
    /// and the loop is done. Returns where the caller goes on from.
    #[inline(always)]
    pub(super) fn leave(&mut self, regs: &mut Regs<'_>) -> Option<usize> {
        let len = regs.stack.len();
        if len == 0 {
            return None;
        }
        self.leave_dynamic(&regs.frame);
        self.frame_stack.pop(regs.frame);
        // SAFETY: the top activation, taken off the vector first and then
        // read out of where it was, its owned field moved out, so that
        // nothing in it is dropped twice whatever happens after.
        //
        // Field by field, and volatile so that the compiler does not merge
        // the reads into wider ones: the activation was just written field
        // by field, and a load wider than the stores it reads waits for them
        // to reach memory rather than taking their values as they go. Moving
        // it out of the vector whole did that, at about 4% of a call.
        unsafe {
            use std::ptr::{addr_of, read_volatile};
            regs.stack.set_len(len - 1);
            let top = regs.stack.as_ptr().add(len - 1);
            regs.bc = read_volatile(addr_of!((*top).bc));
            regs.bc_keep = std::ptr::read(addr_of!((*top).bc_keep));
            regs.func = &*(*regs.bc).func;
            regs.code_ref = read_volatile(addr_of!((*top).code_ref));
            if let Some(ctx) = &*addr_of!((*top).ctx) {
                regs.ctx = *ctx;
            }
            let base = read_volatile(addr_of!((*top).base));
            let layout = read_volatile(addr_of!((*top).layout));
            regs.frame = Frame::without_liveness(base, layout);
            regs.ret_dest = Destination {
                ptr: read_volatile(addr_of!((*top).ret_dest.ptr)),
                tydesc: read_volatile(addr_of!((*top).ret_dest.tydesc)),
            };
            Some(read_volatile(addr_of!((*top).pc)))
        }
    }

    /// The plan for a call, if it has one and it still holds: no body
    /// replaced since it was made, and if a dispatcher is installed, made by
    /// what it said (`CallCache::dispatcher`).
    ///
    /// Within one code epoch no body changes -- bodies are replaced only
    /// between units, and doing so starts a new epoch -- so a call site that
    /// found its callee in this epoch has the callee it would find again.
    ///
    /// Counts the call down, for a site counting calls for the dispatcher.
    #[inline(always)]
    pub(super) fn valid_plan<'c>(&self, call: &'c FastCall) -> Option<Planned<'c>> {
        // SAFETY: see `FastCall::site`: only the slow path writes it, and
        // nothing holds the plan across the slow path.
        let cache = unsafe { &*call.site.as_ptr() }.callee.as_ref()?;
        if cache.epoch != self.code_epoch {
            return None;
        }
        let dispatched = self.call_dispatcher.borrow().is_some();
        match cache.plan.as_ref()? {
            Plan::Native(plan) => Some(Planned::Native(plan)),
            // With no dispatcher to count for or enter compiled code through,
            // a body is interpreted, and compiled code is left to the slow
            // path, which interprets it.
            Plan::Body(plan) if !dispatched => Some(Planned::Body(plan)),
            Plan::Jit(_) if !dispatched => None,
            _ if cache.dispatcher != Some(self.dispatcher_epoch.get()) => None,
            Plan::Body(plan) => match &plan.countdown {
                None => Some(Planned::Body(plan)),
                // Offered, and the site has to ask again.
                Some(countdown) if countdown.run_out() => None,
                Some(countdown) if countdown.tick() => Some(Planned::Offer(countdown.batch())),
                Some(_) => Some(Planned::Body(plan)),
            },
            Plan::Jit(plan) => Some(Planned::Jit(plan)),
        }
    }

    /// Make a planned call: push the callee's frame, write the arguments and
    /// switch to it. Returns where the callee starts.
    #[inline(always)]
    pub(super) fn enter_planned<'r>(
        &mut self,
        regs: &mut Regs<'r>,
        call: &FastCall,
        plan: &BodyPlan,
        base: *mut u8,
        resume: usize,
    ) -> Result<usize, InterpError> {
        // SAFETY: the frame borrows the layout, and the activation the
        // bytecode, from the plan, which outlives the frame: the plan is in a
        // call-site cache of the caller's bytecode, which the caller's own
        // activation keeps alive -- by `bc_keep`, by the plan it was entered
        // through, or, at the bottom, by whoever entered the loop -- and a
        // cache that is replaced while frames may still borrow from it is
        // retired to the interpreter rather than dropped, until no frame is
        // left (`release_retired_call_caches`).
        let mut frame = unsafe { self.frame_stack.push_borrowed(&plan.layout) }?;
        let callee_base = frame.base_ptr();
        match &plan.params {
            PlannedParams::At(params) => for param in params.iter() {
                // SAFETY: `src` was lowered against the caller's frame, which
                // `base` is, and `value` and `copy` are places in the callee's.
                unsafe {
                    let mut ptr = param.src.at(base);
                    if let Some((at, size)) = param.copy {
                        let copy = callee_base.add(at as usize);
                        copy_bytes(ptr, copy, size as usize);
                        ptr = copy;
                    }
                    (callee_base.add(param.value as usize) as *mut crate::value::Value)
                        .write(crate::value::Value { ptr, tydesc: param.tydesc });
                }
            },
            PlannedParams::Recipes(params) => for param in params.iter() {
                // SAFETY: the recipe was made against the caller's frame,
                // which `base` is, and `value` and `copy` are places in the
                // callee's.
                unsafe {
                    let mut arg = self.arg_value(&param.arg, base, &regs.frame, regs.frames);
                    if let Some((at, size)) = param.copy {
                        let copy = callee_base.add(at as usize);
                        copy_bytes(arg.ptr, copy, size as usize);
                        arg.ptr = copy;
                    }
                    (callee_base.add(param.value as usize) as *mut crate::value::Value).write(arg);
                }
            },
        }
        frame.stop_keeping_liveness();
        frame.clear_tracking();
        // The type parameters' descriptors, and the table and storage they
        // make.
        if !plan.descriptors.is_empty() {
            for (i, &r) in plan.descriptors.iter().enumerate() {
                frame.set_type_param(i, Self::resolve_binding(r, &regs.frame));
            }
            self.enter_dynamic(&frame);
        }
        let dest = if call.dest_tydesc.is_null() {
            regs.frame.value_dest(call.dest_id)
        } else {
            // SAFETY: lowered against the caller's frame.
            Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc }
        };
        let callee = Callee {
            frame,
            bc: std::rc::Rc::as_ptr(&plan.bc),
            keep: None,
            // SAFETY: as the frame's layout, and the caller's body holds
            // `code_ref`.
            func: unsafe { &*plan.func },
            code_ref: unsafe { &*plan.code_ref },
            ctx: None,
            dest,
        };
        Ok(self.switch_to(regs, callee, resume))
    }

    /// An argument of a planned call, as its recipe reads it from the caller's
    /// frame, `base`.
    ///
    /// # Safety
    ///
    /// The recipe was made against the frame `base` is.
    #[inline(always)]
    unsafe fn arg_value(
        &self,
        recipe: &ArgRecipe,
        base: *mut u8,
        frame: &Frame,
        frames: &FrameStore,
    ) -> crate::value::Value {
        let mut arg = match recipe.src {
            // SAFETY: the caller's.
            ArgSource::At(loc, tydesc) => crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc },
            ArgSource::Read(operand) => self.read_operand(&operand, frame, frames),
        };
        if let Some(tydesc) = recipe.retype {
            arg.tydesc = tydesc;
        }
        arg
    }

    /// Make a planned call to a rider function: its C words straight from the
    /// arguments, and the call.
    #[inline(never)]
    pub(super) fn call_planned_native(&mut self, regs: &mut Regs<'_>, call: &FastCall, plan: &NativePlan, base: *mut u8) {
        let n = plan.args.len();
        let len = 1 + 2 * n + 2;
        let mut words = [0usize; MAX_C_WORDS];
        words[0] = self.runtime.handle() as usize;
        for (i, recipe) in plan.args.iter().enumerate() {
            // SAFETY: the recipe was made against the caller's frame.
            let arg = unsafe { self.arg_value(recipe, base, &regs.frame, regs.frames) };
            words[1 + 2 * i] = arg.ptr as usize;
            words[2 + 2 * i] = arg.tydesc as usize;
        }
        // A dynamic destination is where its fat pointer says, described by
        // what is there; so is a forwarder's dynamic result.
        let dest = if call.dest_tydesc.is_null() {
            regs.frame.value_dest(call.dest_id)
        } else {
            // SAFETY: the destination was lowered against the caller's frame.
            Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc }
        };
        words[1 + 2 * n] = dest.ptr as usize;
        words[2 + 2 * n] = if plan.dest_tydesc.is_null() { dest.tydesc } else { plan.dest_tydesc } as usize;
        // Then the descriptors the native's signature asks for.
        for (k, &r) in plan.descriptors.iter().enumerate() {
            words[len + k] = Self::resolve_binding(r, &regs.frame) as usize;
        }
        let len = len + plan.descriptors.len();
        // SAFETY: the table registered this as a rider function in the
        // generation the plan was checked against, and holds its code until
        // that changes.
        unsafe { call_c(plan.fn_ptr, &words[..len]) };
    }

    /// Make a planned call into compiled code: the words it is entered with
    /// straight from the arguments, and the call, through the dispatcher.
    #[inline(never)]
    pub(super) fn call_planned_jit(&mut self, regs: &mut Regs<'_>, call: &FastCall, plan: &JitPlan, base: *mut u8) -> Result<(), InterpError> {
        let mut words = [0usize; crate::dispatch::MAX_ENTRY_WORDS];
        let mut len = 0;
        let mut push = |word: usize| {
            words[len] = word;
            len += 1;
        };
        push(self.runtime.handle() as usize);
        if plan.entry.uses_sret {
            // SAFETY: the destination was lowered against the caller's frame.
            push(unsafe { call.dest.at(base) } as usize);
        }
        let mut args = [crate::value::Value { ptr: std::ptr::null_mut(), tydesc: std::ptr::null() };
            crate::dispatch::MAX_ENTRY_WORDS];
        for (i, recipe) in plan.args.iter().enumerate() {
            // SAFETY: the recipe was made against the caller's frame.
            args[i] = unsafe { self.arg_value(recipe, base, &regs.frame, regs.frames) };
            push(args[i].ptr as usize);
        }

        self.with_dispatcher(&regs.ctx, regs.registry, regs.frames, &[], 1,
                |dispatcher, call_ctx| dispatcher.call_compiled(plan.func, plan.entry, &words[..len], call_ctx))
            .expect("a planned call into compiled code is made with its dispatcher installed")
    }

    /// Ask the dispatcher about a loop whose counter ran out, and go on in
    /// compiled code if it says to. Returns whether it did, in which case the
    /// call is over, its result written.
    ///
    /// The frame is the running one, at the loop's header.
    #[inline(never)]
    pub(super) fn loop_hot(&mut self, regs: &mut Regs<'_>, counter: &LoopCounter, base: *mut u8) -> Result<bool, InterpError> {
        use crate::dispatch::SitePolicy;
        // How long to go before asking again when there is nothing to ask:
        // long enough to cost nothing, and a dispatcher may yet be installed.
        const UNASKED: u32 = 1 << 16;
        let countdown = &counter.countdown;
        // A script unit's loops stay in the interpreter: only a function has
        // a frame compiled code can carry on in.
        let (Some(code_ref), Some(_)) = (regs.code_ref, regs.func.function_context()) else {
            countdown.set(u32::MAX);
            return Ok(false);
        };
        let func = crate::dispatch::FuncIdentity::of(code_ref, regs.ctx.unit());
        let body = regs.func;
        let policy = self.with_dispatcher(&regs.ctx, regs.registry, regs.frames, &[], 1,
            |dispatcher, call_ctx| dispatcher.loop_policy(func, body, counter.header, countdown.batch(), call_ctx));
        let entry = match policy.transpose()? {
            None => {
                countdown.set(UNASKED);
                return Ok(false);
            }
            Some(SitePolicy::EveryCall) => {
                countdown.set(1);
                return Ok(false);
            }
            Some(SitePolicy::Count(n)) => {
                countdown.set(n);
                return Ok(false);
            }
            Some(SitePolicy::Interpret) => {
                countdown.set(u32::MAX);
                return Ok(false);
            }
            Some(SitePolicy::Enter(entry)) => entry,
        };
        // The next call to run the loop asks again, and is told the same at
        // once.
        countdown.set(1);
        let words = [self.runtime.handle() as usize, regs.ret_dest.ptr as usize, base as usize];
        let words = if entry.uses_sret { &words[..] } else { &[words[0], words[2]][..] };
        self.with_dispatcher(&regs.ctx, regs.registry, regs.frames, &[], 1,
                |dispatcher, call_ctx| dispatcher.call_compiled(func, entry, words, call_ctx))
            .expect("the dispatcher that gave the entry")
            .map(|()| true)
    }

    /// Pop every frame the loop pushed, on the way out with an error.
    #[cold]
    pub(super) fn unwind(&mut self, regs: &mut Regs<'_>) {
        while let Some(caller) = regs.stack.pop() {
            self.frame_stack.pop(regs.frame);
            // SAFETY: the caller's frame, still on the stack.
            regs.frame = unsafe { Frame::without_liveness(caller.base, caller.layout) };
        }
    }
}

impl IrInterpreter {
    /// The body a call lands on: a module callee from the call site, while
    /// the registry is the one it was found in.
    fn resolve_callee<'r>(
        call: &FastCall,
        code_ref: &CodeRef,
        ctx: &ExecutionContext<'r>,
        registry: &'r FunctionRegistry,
    ) -> &'r IrCodeUnit {
        match code_ref {
            CodeRef::Module { .. } => {
                let current = registry.module_registry_arc();
                let mut site = call.site.borrow_mut();
                let found = &mut site.module_callee;
                match &*found {
                    // SAFETY: the registry the body is in is held, and is the
                    // one this call is made against.
                    Some((held, body)) if std::sync::Arc::ptr_eq(held, current) => unsafe { &**body },
                    _ => {
                        let body = ctx.get_unit(code_ref, registry);
                        *found = Some((std::sync::Arc::clone(current), body as *const IrCodeUnit));
                        body
                    }
                }
            }
            _ => ctx.get_unit(code_ref, registry),
        }
    }

    /// Whether a call's arguments suit a fast call into a native.
    fn suits_native(call: &FastCall, native: &datalove_datafun_ir::NativeContext) -> bool {
        call.args.iter().enumerate().all(|(i, &movable)| {
            match native.param_modes.get(i).copied().unwrap_or(ParamMode::In) {
                ParamMode::Out => false,
                ParamMode::In => movable || native.param_types[i].is_copy(),
                ParamMode::Ref | ParamMode::Mut => true,
            }
        })
    }

    /// The bytecode of a body that only forwards its parameters to a native,
    /// if `callee` is one.
    ///
    /// A forwarder is one call that passes the parameters in order, then
    /// returns what the call returned: most of the
    /// standard library's wrappers around natives. Calling the native from
    /// the forwarder's caller saves a frame and a call.
    fn forwarder(
        &mut self,
        layout: &IrLayout,
        callee: &IrCodeUnit,
        callee_ctx: &ExecutionContext,
        registry: &FunctionRegistry,
    ) -> Option<std::rc::Rc<BcFunction>> {
        let [block] = &callee.blocks[..] else { return None };
        let [Instruction::Call { func, dest, args, .. }] = &block.instructions[..] else {
            return None;
        };
        let in_order = args.len() == layout.param_modes.len()
            && args.iter().enumerate().all(|(i, a)| matches!(a, Operand::Param(p) if p.0 as usize == i));
        let returns = match &block.terminator {
            Terminator::Return { value: None } => true,
            Terminator::Return { value: Some(Operand::Value(v)) } => v == dest,
            _ => false,
        };
        if !in_order || !returns {
            return None;
        }
        let bc = self.bytecode_for(layout, callee);
        let [inner] = &bc.calls[..] else { return None };
        if !matches!(bc.ops.first(), Some(Op::CallFast { site: 0 })) {
            return None;
        }
        let CodeUnitContext::Native(native) = &Self::resolve_callee(inner, func, callee_ctx, registry).context else {
            return None;
        };
        Self::suits_native(inner, native).then_some(bc)
    }

    /// The descriptors a call to `callee` binding `bindings` hands over, as a
    /// site keeps them: the type parameters', a native's undetermined ones,
    /// or, for a forwarder, its native's, through both calls' bindings.
    fn site_descriptors(
        &mut self,
        callee: &IrCodeUnit,
        bindings: &[IrType],
        forwarded: Option<&[datalove_datafun_ir::DescriptorShape]>,
        caller: &IrLayout,
    ) -> Vec<crate::BindingRef> {
        // A wrapper that is not generic may still forward to a generic native,
        // binding its type parameters concretely itself.
        if bindings.is_empty() && forwarded.is_none() {
            return Vec::new();
        }
        let wanted: Vec<IrType> = match &callee.context {
            CodeUnitContext::Native(native) => native.descriptor_shapes.iter()
                .map(|s| s.to_ir_type().substitute(bindings))
                .collect(),
            _ if forwarded.is_some() => {
                let Instruction::Call { bindings: inner, .. } = &callee.blocks[0].instructions[0] else {
                    unreachable!("a forwarder is a call")
                };
                if inner.is_empty() {
                    Vec::new()
                } else {
                    forwarded.unwrap_or_default().iter()
                        .map(|s| s.to_ir_type().substitute(inner).substitute(bindings))
                        .collect()
                }
            }
            _ => bindings.to_vec(),
        };
        wanted.iter().map(|ty| self.binding_ref(ty, caller)).collect()
    }

    /// Call a native from a fast call, with each argument from `arg`.
    ///
    /// # Safety
    ///
    /// `arg` must give what the native's ABI requires.
    #[allow(clippy::too_many_arguments)]
    unsafe fn call_native(
        &self,
        call: &FastCall,
        address: usize,
        native: &datalove_datafun_ir::NativeContext,
        dest: Destination,
        shapes: &[*const rtdt::TyDesc],
        mut arg: impl FnMut(&Self, usize, ParamMode) -> crate::value::Value,
    ) -> Result<(), InterpError> {
        let mode = |i: usize| native.param_modes.get(i).copied().unwrap_or(ParamMode::In);
        let n = call.args.len();
        let target = {
            let generation = self.native_table.generation();
            let mut site = call.site.borrow_mut();
            let found = &mut site.native;
            match &*found {
                Some((at, seen, target)) if *at == address && *seen == generation => target.clone(),
                _ => {
                    let target = self.native_table.lookup(native.symbol())?;
                    *found = Some((address, generation, target.clone()));
                    target
                }
            }
        };
        if let NativeTarget::C(fn_ptr) = target {
            let mut words = [0usize; MAX_C_WORDS];
            let len = crate::native::c_words(
                &mut words, self.runtime.handle(), (0..n).map(|i| arg(self, i, mode(i))), dest, shapes)?;
            // SAFETY: the table registered this as a rider function and holds
            // its code until its generation changes.
            unsafe { call_c(fn_ptr, &words[..len]) };
            return Ok(());
        }
        // On the stack when there are few, which is nearly always: a list per
        // call was an allocation per call.
        const ON_STACK: usize = 8;
        let mut stack = [crate::value::Value { ptr: std::ptr::null_mut(), tydesc: std::ptr::null() }; ON_STACK];
        let heap: Vec<crate::value::Value>;
        let args: &[crate::value::Value] = if n <= ON_STACK {
            for (i, slot) in stack.iter_mut().enumerate().take(n) {
                *slot = arg(self, i, mode(i));
            }
            &stack[..n]
        } else {
            heap = (0..n).map(|i| arg(self, i, mode(i))).collect();
            &heap
        };
        let NativeTarget::Rust(f) = target else { unreachable!("a C native was called above") };
        f(self.runtime.handle(), args, dest, shapes)
    }

    /// The plan for a call whose arguments suit it: a body, a rider function,
    /// or a forwarder to one.
    ///
    /// Each argument is read as `fast_call` would read it, so the plan makes
    /// the same call: from where the lowering resolved it, or as the IR walker
    /// reads its operand; then, for a forwarder, as the forwarder's own call
    /// reads its parameter.
    #[allow(clippy::too_many_arguments)]
    fn make_plan(
        &mut self,
        call: &FastCall,
        ir_args: &[Operand],
        callee: &IrCodeUnit,
        code_ref: &CodeRef,
        callee_ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        layout: Option<&std::rc::Rc<IrLayout>>,
        forward: Option<&std::rc::Rc<BcFunction>>,
        policy: Option<crate::dispatch::SitePolicy>,
        descriptors: &std::rc::Rc<[crate::BindingRef]>,
    ) -> Option<Plan> {
        use crate::dispatch::SitePolicy;
        // The argument as the call reads it.
        let read = |i: usize| match &call.resolved {
            Some(resolved) => ArgRecipe { src: ArgSource::At(resolved[i].0, resolved[i].1), retype: None },
            None => ArgRecipe { src: ArgSource::Read(ir_args[i]), retype: None },
        };
        let native_plan = |this: &Self, native: &datalove_datafun_ir::NativeContext, args: Box<[ArgRecipe]>, dest_tydesc| {
            let NativeTarget::C(fn_ptr) = this.native_table.lookup(native.symbol()).ok()? else { return None };
            (1 + 2 * args.len() + 2 + descriptors.len() <= MAX_C_WORDS).then(|| Plan::Native(NativePlan {
                fn_ptr, generation: this.native_table.generation(), args, dest_tydesc,
                descriptors: std::rc::Rc::clone(descriptors),
            }))
        };
        match (layout, forward) {
            // A rider function.
            (None, _) => {
                let CodeUnitContext::Native(native) = &callee.context else { unreachable!("a body without a layout is a native") };
                let args = (0..call.args.len()).map(read).collect();
                native_plan(self, native, args, call.dest_tydesc)
            }
            // A forwarder: each argument as its frame would hold it -- an owned
            // one with its own descriptor -- then as its call reads that.
            (Some(layout), Some(wrapper)) => {
                let inner = &wrapper.calls[0];
                let Instruction::Call { func: inner_ref, .. } = &callee.blocks[0].instructions[0] else {
                    unreachable!("a forwarder is a call")
                };
                let native_body = Self::resolve_callee(inner, inner_ref, callee_ctx, registry);
                let CodeUnitContext::Native(native) = &native_body.context else {
                    unreachable!("a forwarder calls a native")
                };
                let args = (0..call.args.len()).map(|i| {
                    let mut recipe = read(i);
                    // A dynamic parameter keeps the descriptor it came with.
                    if matches!(layout.param_modes[i], ParamMode::In | ParamMode::Out)
                        && !layout.param_tydescs[i].is_null()
                    {
                        recipe.retype = Some(layout.param_tydescs[i]);
                    }
                    if let Some(resolved) = &inner.resolved {
                        recipe.retype = Some(resolved[i].1);
                    }
                    recipe
                }).collect();
                native_plan(self, native, args, inner.dest_tydesc)
            }
            // A body the dispatcher wants to see every call to.
            (Some(_), None) if matches!(policy, Some(SitePolicy::EveryCall)) => None,
            // A body compiled, entered as the dispatcher would enter it from
            // the call's arguments: each as the caller has it, descriptor and
            // all, which is what a call offered to it is handed.
            (Some(layout), None) if matches!(policy, Some(SitePolicy::Enter(_))) => {
                let Some(SitePolicy::Enter(entry)) = policy else { unreachable!() };
                let args: Box<[ArgRecipe]> = (0..layout.param_modes.len()).map(read).collect();
                let width = 1 + entry.uses_sret as usize + args.len();
                (width <= crate::dispatch::MAX_ENTRY_WORDS)
                    .then(|| Plan::Jit(JitPlan {
                        func: crate::dispatch::FuncIdentity::of(code_ref, callee_ctx.unit()),
                        entry, args,
                    }))
            }
            // A body, whose frame holds an owned argument with its own
            // descriptor and a borrowed one with the argument's.
            (Some(layout), None) => Some(Plan::Body(BodyPlan {
                descriptors: std::rc::Rc::clone(descriptors),
                countdown: match policy {
                    Some(SitePolicy::Count(n)) => Some(Countdown::new(n)),
                    _ => None,
                },
                layout: std::rc::Rc::clone(layout),
                bc: self.bytecode_for(layout, callee),
                func: callee,
                code_ref,
                params: match &call.resolved {
                    Some(resolved) => PlannedParams::At(layout.param_modes.iter().enumerate().map(|(i, mode)| AtParam {
                        src: resolved[i].0,
                        value: layout.param_offsets[i],
                        // A dynamic parameter keeps the argument's.
                        tydesc: match mode {
                            ParamMode::In | ParamMode::Out if !layout.param_tydescs[i].is_null() => layout.param_tydescs[i],
                            _ => resolved[i].1,
                        },
                        copy: layout.param_copies[i],
                    }).collect()),
                    None => PlannedParams::Recipes(layout.param_modes.iter().enumerate().map(|(i, mode)| {
                        let mut arg = read(i);
                        if matches!(mode, ParamMode::In | ParamMode::Out) && !layout.param_tydescs[i].is_null() {
                            arg.retype = Some(layout.param_tydescs[i]);
                        }
                        PlannedParam { arg, value: layout.param_offsets[i], copy: layout.param_copies[i] }
                    }).collect()),
                },
            })),
        }
    }

    /// Make a fast call, or say the general path has to.
    ///
    /// Does what `execute_call_site` does for a call whose arguments are in
    /// this frame, with no `out` parameter among them and none moved that the
    /// frame would have to record. A call to a body while a dispatcher is
    /// installed is left to the general path, which offers it; what the
    /// dispatcher says about such calls is what the site's plan follows.
    ///
    /// A native is called here. A bytecode body is not run here: its frame is
    /// pushed and entered and handed back, for the loop to run without
    /// recursing.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub(super) unsafe fn fast_call<'r>(
        &mut self,
        call: &FastCall,
        base: *mut u8,
        frame: &mut Frame,
        ctx: &ExecutionContext<'r>,
        registry: &'r FunctionRegistry,
        frames: &mut FrameStore,
        caller: &'r IrCodeUnit,
    ) -> Result<Called<'r>, InterpError> {
        // A dispatcher is asked how this site is to make its calls to a body,
        // which the plan made here then follows; this call it is offered.
        let dispatched = self.call_dispatcher.borrow().is_some();
        let dispatcher_epoch = self.dispatcher_epoch.get();
        let Instruction::Call { func: code_ref, args: ir_args, bindings, .. } =
            &caller.blocks[call.block as usize].instructions[call.index as usize] else {
            unreachable!("a fast call is a call")
        };
        let callee = Self::resolve_callee(call, code_ref, ctx, registry);
        let callee_ctx = ctx.for_callee(code_ref, registry);
        let address = callee as *const IrCodeUnit as usize;
        let (layout, suits, forward, descriptors) = {
            let mut site = call.site.borrow_mut();
            let cache = &mut site.callee;
            match &*cache {
                Some(c) if c.address == address && c.epoch == self.code_epoch
                    && c.generation == self.native_table.generation()
                    && !(dispatched && c.needs_policy(dispatcher_epoch)) =>
                {
                    (c.layout.clone(), c.suits, c.forward.clone(), c.descriptors.clone())
                }
                _ => {
                    let (layout, suits, forward) = match &callee.context {
                        CodeUnitContext::Native(native) => (None, Self::suits_native(call, native), None),
                        _ => {
                            let identity = crate::dispatch::FuncIdentity::of(code_ref, ctx.unit());
                            let layout = self.layout_cache.get_or_compute(identity, callee, &mut self.tydesc_table);
                            let suits = call.args.iter().enumerate().all(|(i, &movable)| {
                                match layout.param_modes[i] {
                                    ParamMode::Out => false,
                                    ParamMode::In => movable || !layout.param_moves[i],
                                    ParamMode::Ref | ParamMode::Mut => true,
                                }
                            });
                            let forward = self.forwarder(&layout, callee, &callee_ctx, registry);
                            (Some(layout), suits, forward)
                        }
                    };
                    // A forwarder's native, for the descriptors it asks for.
                    let forwarded = forward.as_ref().map(|bc| {
                        let Instruction::Call { func: inner_ref, .. } = &callee.blocks[0].instructions[0] else {
                            unreachable!("a forwarder is a call")
                        };
                        let native_body = Self::resolve_callee(&bc.calls[0], inner_ref, &callee_ctx, registry);
                        match &native_body.context {
                            CodeUnitContext::Native(native) => native.descriptor_shapes.clone(),
                            _ => unreachable!("a forwarder calls a native"),
                        }
                    });
                    let descriptors: std::rc::Rc<[crate::BindingRef]> =
                        self.site_descriptors(callee, bindings, forwarded.as_deref(), frame.layout()).into();
                    // A call binding type parameters is planned only where it
                    // lands on a native, which takes its descriptors from the
                    // plan; a body is handed them on the fast path.
                    // Under a dispatcher a body's plan may be compiled code's,
                    // which takes no type parameter descriptors from a plan.
                    let natively = layout.is_none() || forward.is_some();
                    let plannable = suits
                        && (bindings.is_empty() || natively || !dispatched)
                        && !matches!(code_ref, CodeRef::External { .. });
                    let policy = (plannable && dispatched && layout.is_some() && forward.is_none()).then(|| {
                        let identity = crate::dispatch::FuncIdentity::of(code_ref, ctx.unit());
                        self.call_dispatcher.borrow_mut().as_mut()
                            .expect("installed")
                            .site_policy(identity, callee)
                    });
                    let plan = if plannable {
                        self.make_plan(call, ir_args, callee, code_ref, &callee_ctx, registry, layout.as_ref(), forward.as_ref(), policy, &descriptors)
                    } else {
                        None
                    };
                    let fresh = CallCache {
                        address, epoch: self.code_epoch, generation: self.native_table.generation(),
                        layout: layout.clone(), suits, forward: forward.clone(), plan,
                        descriptors: descriptors.clone(),
                        dispatcher: dispatched.then_some(dispatcher_epoch),
                    };
                    // A planned call's frame borrows its layout and bytecode
                    // from the plan, so a replaced one is kept.
                    if let Some(old) = cache.replace(fresh) {
                        self.retired_call_caches.push(old);
                    }
                    (layout, suits, forward, descriptors)
                }
            }
        };
        // A body goes to the dispatcher, which a native, or a forwarder to
        // one, never does.
        if !suits || (dispatched && layout.is_some() && forward.is_none()) {
            return Ok(Called::General);
        }
        let dest = if call.dest_tydesc.is_null() {
            frame.value_dest(call.dest_id)
        } else {
            Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc }
        };

        let read = |this: &Self, i: usize, frame: &Frame| {
            if let Some(resolved) = &call.resolved {
                let (loc, tydesc) = resolved[i];
                // SAFETY: lowered against this frame's layout.
                return crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc };
            }
            this.read_operand(&ir_args[i], frame, frames)
        };

        if let (Some(wrapper), Some(layout)) = (&forward, &layout) {
            // The forwarder's own call, made from here: each argument as the
            // forwarder's frame would hold it -- an owned one with the
            // forwarder's descriptor, a borrowed one with the one it came
            // with -- then as its call would read it, and the result where
            // the forwarder would have copied it, in its shape.
            let inner = &wrapper.calls[0];
            let Instruction::Call { func: inner_ref, .. } = &callee.blocks[0].instructions[0] else {
                unreachable!("a forwarder is a call")
            };
            let native_body = Self::resolve_callee(inner, inner_ref, &callee_ctx, registry);
            let CodeUnitContext::Native(native) = &native_body.context else {
                unreachable!("a forwarder calls a native, and its body is fixed while the registry is")
            };
            // A dynamic result is the forwarder's, which is this call's.
            let inner_tydesc = if inner.dest_tydesc.is_null() { dest.tydesc } else { inner.dest_tydesc };
            let inner_dest = Destination { ptr: dest.ptr, tydesc: inner_tydesc };
            // A generic forwarder's native is handed what its call bound, with
            // what this call bound the forwarder's own type parameters to
            // substituted in.
            let supplied: Vec<*const rtdt::TyDesc> = descriptors.iter()
                .map(|&r| Self::resolve_binding(r, frame))
                .collect();
            // SAFETY: the arguments are read as the forwarder would read them.
            unsafe { self.call_native(
                inner, native_body as *const IrCodeUnit as usize, native, inner_dest, &supplied,
                |this, i, _| {
                    let mut val = read(this, i, frame);
                    // A dynamic parameter keeps the descriptor it came with.
                    if matches!(layout.param_modes[i], ParamMode::In | ParamMode::Out)
                        && !layout.param_tydescs[i].is_null()
                    {
                        val.tydesc = layout.param_tydescs[i];
                    }
                    match &inner.resolved {
                        Some(resolved) => crate::value::Value { ptr: val.ptr, tydesc: resolved[i].1 },
                        None => val,
                    }
                },
            ) }?;
            return Ok(Called::Done);
        }

        let Some(layout) = layout else {
            // A native: no frame, just the arguments, and a descriptor for
            // each type parameter no argument determines, for what this call
            // bound it to.
            let CodeUnitContext::Native(native) = &callee.context else { unreachable!() };
            let supplied: Vec<*const rtdt::TyDesc> = descriptors.iter()
                .map(|&r| Self::resolve_binding(r, frame))
                .collect();
            // SAFETY: the arguments are read from this frame as lowered.
            unsafe { self.call_native(call, address, native, dest, &supplied, |this, i, _| read(this, i, frame)) }?;
            return Ok(Called::Done);
        };

        let mut callee_frame = self.frame_stack.push(std::rc::Rc::clone(&layout))?;
        for i in 0..call.args.len() {
            callee_frame.set_param(i, read(self, i, frame));
        }
        for (i, &r) in descriptors.iter().enumerate() {
            callee_frame.set_type_param(i, Self::resolve_binding(r, frame));
        }
        callee_frame.enter();
        self.enter_dynamic(&callee_frame);
        callee_frame.stop_keeping_liveness();
        // The caller's frame keeps no record of an argument moved into a fast
        // call: those with tracking bytes do not suit it.
        let bc = self.bytecode_for(&layout, callee);
        Ok(Called::Enter(Callee {
            frame: callee_frame,
            bc: std::rc::Rc::as_ptr(&bc),
            keep: Some(bc),
            func: callee,
            code_ref,
            ctx: Some(callee_ctx),
            dest,
        }))
    }
}
