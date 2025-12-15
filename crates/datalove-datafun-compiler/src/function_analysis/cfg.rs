//! Control flow graph construction.

use rmx::prelude::*;
use crate::ast::{Statement, StmtFun, ExprFun, ExprFunKind};
use super::{StmtId, BlockId};

/// Control flow graph for a function.
#[salsa::tracked]
pub struct ControlFlowGraph<'db> {
    #[returns(ref)]
    pub blocks: Vec<BasicBlock>,
    #[returns(ref)]
    pub edges: Vec<ControlFlowEdge>,
    /// Map from StmtId to Statement for execution.
    #[returns(ref)]
    pub stmt_map: Vec<Statement<'db>>,
}

/// A basic block in the CFG.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct BasicBlock {
    pub block_id: BlockId,
    pub statements: Vec<StmtId>,
    pub terminator: Terminator,
}

/// Terminator instruction for a basic block.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub enum Terminator {
    /// Normal function return.
    Return,
    /// Conditional branch (if-statement).
    Branch {
        /// StmtId of the if-statement containing the condition to evaluate.
        condition_stmt: StmtId,
        then_block: BlockId,
        else_block: BlockId,
    },
    /// Unconditional jump.
    Goto(BlockId),
    /// Early return from ? or ! operator.
    TryReturn,
    /// Jump to loop header (continue).
    LoopContinue(BlockId),
    /// Jump to after loop (break).
    LoopBreak(BlockId),
}

/// Edge in the control flow graph.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct ControlFlowEdge {
    pub from: BlockId,
    pub to: BlockId,
}

/// Builder for constructing a CFG.
pub struct CfgBuilder<'db> {
    blocks: Vec<BasicBlock>,
    edges: Vec<ControlFlowEdge>,
    stmts: Vec<Statement<'db>>,
    next_block_id: u32,
    next_stmt_id: u32,
    /// Stack of (header_block, exit_block) for nested loops.
    loop_stack: Vec<(BlockId, BlockId)>,
}

/// Build the CFG for a function.
#[salsa::tracked]
pub fn build_cfg<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
) -> ControlFlowGraph<'db> {
    let mut builder = CfgBuilder::new();

    // Create entry block.
    let entry_block = builder.alloc_block_id();

    // Build CFG from function body.
    let (_exit_block, _created) = builder.build_statements(db, func.body(db), entry_block);

    ControlFlowGraph::new(db, builder.blocks, builder.edges, builder.stmts)
}

impl<'db> CfgBuilder<'db> {
    /// Create a new CFG builder.
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            edges: Vec::new(),
            stmts: Vec::new(),
            next_block_id: 0,
            next_stmt_id: 0,
            loop_stack: Vec::new(),
        }
    }

    /// Allocate a new block ID.
    fn alloc_block_id(&mut self) -> BlockId {
        let id = BlockId(self.next_block_id);
        self.next_block_id += 1;
        id
    }

    /// Allocate a new statement ID and store the statement.
    fn alloc_stmt_id(&mut self, stmt: Statement<'db>) -> StmtId {
        let id = StmtId(self.next_stmt_id);
        self.next_stmt_id += 1;
        self.stmts.push(stmt);
        id
    }

    /// Add a block to the CFG.
    fn add_block(&mut self, block: BasicBlock) {
        self.blocks.push(block);
    }

    /// Add an edge to the CFG.
    fn add_edge(&mut self, from: BlockId, to: BlockId) {
        self.edges.push(ControlFlowEdge { from, to });
    }

    /// Build CFG for a list of statements, starting at the given block.
    /// Returns (exit_block_id, block_was_created).
    /// - exit_block_id: block where control flow exits (None if all paths diverge)
    /// - block_was_created: true if a block with exit_block_id was already created
    fn build_statements(
        &mut self,
        db: &'db dyn crate::Db,
        stmts: &[Statement<'db>],
        start_block: BlockId,
    ) -> (Option<BlockId>, bool) {
        let mut current_block = start_block;
        let mut current_stmts = Vec::new();

        for stmt in stmts {
            match stmt {
                Statement::Let(let_stmt) => {
                    // Simple statement - add to current block.
                    let stmt_id = self.alloc_stmt_id(stmt.clone());
                    current_stmts.push(stmt_id);

                    // Check if the value expression contains try operators.
                    if self.expr_may_return_early(db, let_stmt.value(db)) {
                        // Expression may return early via ? or !.
                        // Create blocks for early return and continuation.
                        let early_return_block = self.alloc_block_id();
                        let continue_block = self.alloc_block_id();

                        // Current block terminates with branch to early return or continuation.
                        // Note: This is simplified. A full implementation would split the
                        // expression evaluation into multiple blocks to handle the exact
                        // point where the try operator is evaluated.
                        // For try operators, condition_stmt refers to the let statement with the try.
                        self.add_block(BasicBlock {
                            block_id: current_block,
                            statements: current_stmts.clone(),
                            terminator: Terminator::Branch {
                                condition_stmt: stmt_id,
                                then_block: continue_block,
                                else_block: early_return_block,
                            },
                        });

                        // Early return block terminates with TryReturn.
                        self.add_block(BasicBlock {
                            block_id: early_return_block,
                            statements: Vec::new(),
                            terminator: Terminator::TryReturn,
                        });

                        // Add edges.
                        self.add_edge(current_block, continue_block);
                        self.add_edge(current_block, early_return_block);

                        // Continue from the continuation block.
                        current_block = continue_block;
                        current_stmts = Vec::new();
                    }
                }

                Statement::Ret(_ret_stmt) => {
                    // Return statement terminates the current block.
                    let stmt_id = self.alloc_stmt_id(stmt.clone());
                    current_stmts.push(stmt_id);

                    self.add_block(BasicBlock {
                        block_id: current_block,
                        statements: current_stmts,
                        terminator: Terminator::Return,
                    });

                    // No continuation after return.
                    return (None, true);
                }

                Statement::If(if_stmt) => {
                    // If-statement creates a branch.
                    let stmt_id = self.alloc_stmt_id(stmt.clone());
                    current_stmts.push(stmt_id);

                    // Create blocks for then and else branches.
                    let then_block = self.alloc_block_id();
                    let else_block = self.alloc_block_id();
                    let join_block = self.alloc_block_id();

                    // Current block terminates with a branch.
                    self.add_block(BasicBlock {
                        block_id: current_block,
                        statements: current_stmts.clone(),
                        terminator: Terminator::Branch {
                            condition_stmt: stmt_id,
                            then_block,
                            else_block,
                        },
                    });

                    // Add edges to then and else blocks.
                    self.add_edge(current_block, then_block);
                    self.add_edge(current_block, else_block);

                    // Build then branch.
                    let (then_exit, then_created) = self.build_statements(db, if_stmt.then_body(db), then_block);
                    if let Some(then_exit_block) = then_exit {
                        // Then branch doesn't return - connect to join block.
                        self.add_edge(then_exit_block, join_block);
                        if !then_created {
                            // Block wasn't created yet, create it now.
                            self.add_block(BasicBlock {
                                block_id: then_exit_block,
                                statements: Vec::new(),
                                terminator: Terminator::Goto(join_block),
                            });
                        } else {
                            // Block was created but has wrong terminator (Return).
                            // We need to fix the terminator to Goto(join_block).
                            // Find and update the block.
                            if let Some(block) = self.blocks.iter_mut().find(|b| b.block_id == then_exit_block) {
                                block.terminator = Terminator::Goto(join_block);
                            }
                        }
                    }

                    // Build else branch (if present).
                    if let Some(else_body) = if_stmt.else_body(db) {
                        let (else_exit, else_created) = self.build_statements(db, else_body, else_block);
                        if let Some(else_exit_block) = else_exit {
                            // Else branch doesn't return - connect to join block.
                            self.add_edge(else_exit_block, join_block);
                            if !else_created {
                                // Block wasn't created yet, create it now.
                                self.add_block(BasicBlock {
                                    block_id: else_exit_block,
                                    statements: Vec::new(),
                                    terminator: Terminator::Goto(join_block),
                                });
                            } else {
                                // Block was created but has wrong terminator (Return).
                                // Fix the terminator to Goto(join_block).
                                if let Some(block) = self.blocks.iter_mut().find(|b| b.block_id == else_exit_block) {
                                    block.terminator = Terminator::Goto(join_block);
                                }
                            }
                        }
                    } else {
                        // No else branch - fall through to join block.
                        self.add_edge(else_block, join_block);
                        self.add_block(BasicBlock {
                            block_id: else_block,
                            statements: Vec::new(),
                            terminator: Terminator::Goto(join_block),
                        });
                    }

                    // Continue from join block.
                    // Create the join block immediately (even if empty) since edges point to it.
                    current_block = join_block;
                    current_stmts = Vec::new();
                }

                Statement::Loop(loop_stmt) => {
                    // Loop creates a back-edge for continue and exit-edge for break.
                    let loop_header = self.alloc_block_id();
                    let loop_exit = self.alloc_block_id();

                    // Current block jumps to loop header.
                    self.add_block(BasicBlock {
                        block_id: current_block,
                        statements: current_stmts.clone(),
                        terminator: Terminator::Goto(loop_header),
                    });
                    self.add_edge(current_block, loop_header);

                    // Push loop context for break/continue.
                    self.loop_stack.push((loop_header, loop_exit));

                    // Build loop body.
                    let (body_exit, body_created) = self.build_statements(db, loop_stmt.body(db), loop_header);

                    // Pop loop context.
                    self.loop_stack.pop();

                    // Connect loop body end back to header (unless body ended with break/return).
                    if let Some(exit_block) = body_exit {
                        self.add_edge(exit_block, loop_header);
                        if !body_created {
                            // Block wasn't created yet.
                            self.add_block(BasicBlock {
                                block_id: exit_block,
                                statements: Vec::new(),
                                terminator: Terminator::Goto(loop_header),
                            });
                        } else {
                            // Update terminator to loop back.
                            if let Some(block) = self.blocks.iter_mut().find(|b| b.block_id == exit_block) {
                                // Only update if it's the default Return terminator.
                                if matches!(block.terminator, Terminator::Return) {
                                    block.terminator = Terminator::Goto(loop_header);
                                }
                            }
                        }
                    }

                    // Continue from loop exit block.
                    current_block = loop_exit;
                    current_stmts = Vec::new();
                }

                Statement::Break(_) => {
                    // Break jumps to innermost loop's exit block.
                    if let Some(&(_, loop_exit)) = self.loop_stack.last() {
                        self.add_block(BasicBlock {
                            block_id: current_block,
                            statements: current_stmts.clone(),
                            terminator: Terminator::LoopBreak(loop_exit),
                        });
                        self.add_edge(current_block, loop_exit);
                        // Break terminates this path.
                        return (None, true);
                    }
                    // Break outside loop - should be caught by typechecker.
                }

                Statement::Continue(_) => {
                    // Continue jumps to innermost loop's header block.
                    if let Some(&(loop_header, _)) = self.loop_stack.last() {
                        self.add_block(BasicBlock {
                            block_id: current_block,
                            statements: current_stmts.clone(),
                            terminator: Terminator::LoopContinue(loop_header),
                        });
                        self.add_edge(current_block, loop_header);
                        // Continue terminates this path.
                        return (None, true);
                    }
                    // Continue outside loop - should be caught by typechecker.
                }

                Statement::Fun(_) => {
                    // Nested functions not yet supported.
                }

                Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                    // No control flow impact.
                }
            }
        }

        // Create a final block for current_block (even if empty).
        // This is necessary because edges may already point to current_block
        // (e.g., join_block after if-statement), and a block may not have been
        // created yet if we just have simple statements.
        // Check for duplicates and merge if needed.
        if let Some(existing) = self.blocks.iter_mut().find(|b| b.block_id == current_block) {
            // Block already exists - shouldn't normally happen, but handle it.
            // Update its statements if we have more.
            if !current_stmts.is_empty() {
                existing.statements.extend(current_stmts);
            }
        } else {
            // Block doesn't exist - create it.
            self.add_block(BasicBlock {
                block_id: current_block,
                statements: current_stmts,
                terminator: Terminator::Return, // Implicit return at end
            });
        }
        (Some(current_block), true)
    }

    /// Check if an expression may return early (contains ? or !).
    fn expr_may_return_early(&self, db: &'db dyn crate::Db, expr: ExprFun<'db>) -> bool {
        match expr.expr(db) {
            ExprFunKind::TryOption(_) | ExprFunKind::TryResult(_) => true,
            ExprFunKind::BinOp(binop) => {
                self.expr_may_return_early(db, binop.lhs(db))
                    || self.expr_may_return_early(db, binop.rhs(db))
            }
            ExprFunKind::UnaryOp(unary) => self.expr_may_return_early(db, unary.operand(db)),
            ExprFunKind::FunctionCall(call) => {
                call.args(db).iter().any(|arg| self.expr_may_return_early(db, *arg))
            }
            ExprFunKind::Tuple(tuple) => {
                tuple.elements(db).iter().any(|elem| self.expr_may_return_early(db, *elem))
            }
            _ => false,
        }
    }
}

impl<'db> ControlFlowGraph<'db> {
    /// Get a basic block by its ID.
    pub fn get_block(&self, db: &'db dyn crate::Db, block_id: BlockId) -> Option<&BasicBlock> {
        self.blocks(db).iter().find(|b| b.block_id == block_id)
    }

    /// Get a statement by its ID.
    pub fn get_stmt(&self, db: &'db dyn crate::Db, stmt_id: StmtId) -> Option<&Statement<'db>> {
        self.stmt_map(db).get(stmt_id.0 as usize)
    }

    /// Get all edges from a given block.
    pub fn outgoing_edges(&self, db: &'db dyn crate::Db, from: BlockId) -> Vec<&ControlFlowEdge> {
        self.edges(db).iter().filter(|e| e.from == from).collect()
    }

    /// Get all edges to a given block.
    pub fn incoming_edges(&self, db: &'db dyn crate::Db, to: BlockId) -> Vec<&ControlFlowEdge> {
        self.edges(db).iter().filter(|e| e.to == to).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bct::input::Source;

    /// Helper to parse a function from source code.
    fn parse_function<'db>(db: &'db dyn crate::Db, source_code: &str) -> crate::ast::StmtFun<'db> {
        let source = Source::new(db, S(source_code));
        let script = crate::parser::parse_for_test(db, source);
        let statements = script.statements(db);

        // Find the first function statement.
        for stmt in statements {
            if let crate::ast::Statement::Fun(fun) = stmt {
                return *fun;
            }
        }

        panic!("No function found in source code");
    }

    #[test]
    fn test_cfg_builder_creation() {
        let builder: CfgBuilder = CfgBuilder::new();
        assert_eq!(builder.blocks.len(), 0);
        assert_eq!(builder.edges.len(), 0);
        assert_eq!(builder.stmts.len(), 0);
        assert_eq!(builder.next_block_id, 0);
        assert_eq!(builder.next_stmt_id, 0);
    }

    #[test]
    fn test_block_id_allocation() {
        let mut builder: CfgBuilder = CfgBuilder::new();
        let id1 = builder.alloc_block_id();
        let id2 = builder.alloc_block_id();
        assert_eq!(id1.0, 0);
        assert_eq!(id2.0, 1);
    }

    // test_stmt_id_allocation removed - alloc_stmt_id now requires a statement

    #[test]
    fn test_simple_linear_function() {
        let ref db = crate::Database::default();
        let source = r#"
fun test()
    let x = @42
    let y = @100
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        // Should have one basic block (entry block).
        let blocks = cfg.blocks(db);
        assert_eq!(blocks.len(), 1);

        // Entry block should have statements and Return terminator.
        assert_eq!(blocks[0].block_id.0, 0);
        assert_eq!(blocks[0].statements.len(), 3); // let x, let y, ret
        assert!(matches!(blocks[0].terminator, Terminator::Return));

        // No edges (single block).
        assert_eq!(cfg.edges(db).len(), 0);
    }

    #[test]
    fn test_if_with_else() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: @u32)
    if x
        let a = @1
    else
        let b = @2
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        let blocks = cfg.blocks(db);

        // Expected blocks:
        // 0: entry (if condition) -> branches to 1 (then) and 2 (else)
        // 1: then block (let a) -> goto 3
        // 2: else block (let b) -> goto 3
        // 3: join block (ret @0)

        // Check we have the expected blocks (4 total).
        assert!(blocks.len() >= 4, "Expected at least 4 blocks, got {}", blocks.len());

        // Entry block should have Branch terminator.
        let entry = &blocks[0];
        assert!(matches!(entry.terminator, Terminator::Branch { .. }));

        // Should have edges from entry to then and else blocks.
        let edges = cfg.edges(db);
        assert!(edges.len() >= 4, "Expected at least 4 edges, got {}", edges.len());
    }

    #[test]
    fn test_if_without_else() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: @u32)
    if x
        let a = @1
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        let blocks = cfg.blocks(db);

        // Expected blocks:
        // 0: entry (if condition) -> branches to 1 (then) and 2 (else/empty)
        // 1: then block (let a) -> goto 3
        // 2: else block (empty) -> goto 3
        // 3: join block (ret @0)

        assert!(blocks.len() >= 3, "Expected at least 3 blocks, got {}", blocks.len());

        // Entry block should have Branch terminator.
        let entry = &blocks[0];
        assert!(matches!(entry.terminator, Terminator::Branch { .. }));
    }

    #[test]
    fn test_explicit_return() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: @u32)
    if x
        ret @1
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        let blocks = cfg.blocks(db);

        // The then branch should have a Return terminator (ret @1).
        // Look for a block with Return terminator.
        let has_return = blocks.iter().any(|b| matches!(b.terminator, Terminator::Return));
        assert!(has_return, "Expected to find at least one Return terminator");
    }

    #[test]
    fn test_try_operator() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: ?@u32): !@u32
    let y = x?
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        let blocks = cfg.blocks(db);

        // Should have blocks for:
        // - Entry (let y = x?)
        // - Early return path (TryReturn)
        // - Continuation path (ret @0)

        // Look for a TryReturn terminator.
        let has_try_return = blocks.iter().any(|b| matches!(b.terminator, Terminator::TryReturn));
        assert!(has_try_return, "Expected to find TryReturn terminator for try operator");

        // Should have at least 3 blocks.
        assert!(blocks.len() >= 3, "Expected at least 3 blocks for try operator, got {}", blocks.len());
    }

    #[test]
    fn test_nested_if() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: @u32, y: @u32)
    if x
        if y
            let a = @1
        end if
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        let blocks = cfg.blocks(db);

        // Nested ifs create multiple branch points.
        // Should have multiple Branch terminators.
        let branch_count = blocks.iter().filter(|b| matches!(b.terminator, Terminator::Branch { .. })).count();
        assert!(branch_count >= 2, "Expected at least 2 Branch terminators for nested if, got {}", branch_count);
    }

    #[test]
    fn test_cfg_queries() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: @u32)
    if x
        let a = @1
    else
        let b = @2
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let cfg = build_cfg(db, func);

        // Test get_block.
        let block_0 = cfg.get_block(db, BlockId(0));
        assert!(block_0.is_some());

        // Test outgoing_edges.
        let outgoing = cfg.outgoing_edges(db, BlockId(0));
        assert!(outgoing.len() > 0, "Expected outgoing edges from entry block");

        // Test incoming_edges.
        if let Some(Terminator::Branch { then_block, .. }) = block_0.map(|b| &b.terminator) {
            let incoming = cfg.incoming_edges(db, *then_block);
            assert!(incoming.len() > 0, "Expected incoming edges to then block");
        }
    }
}
