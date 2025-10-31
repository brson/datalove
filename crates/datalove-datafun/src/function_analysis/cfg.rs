//! Control flow graph construction.

use rmx::prelude::*;
use crate::ast::{Statement, StmtFun, StmtRet, StmtIf, ExprFun, ExprFunKind};
use super::{StmtId, BlockId};

/// Control flow graph for a function.
#[salsa::tracked]
pub struct ControlFlowGraph<'db> {
    #[returns(ref)]
    pub blocks: Vec<BasicBlock>,
    #[returns(ref)]
    pub edges: Vec<ControlFlowEdge>,
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
    Branch { then_block: BlockId, else_block: BlockId },
    /// Unconditional jump.
    Goto(BlockId),
    /// Early return from ? or ! operator.
    TryReturn,
}

/// Edge in the control flow graph.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct ControlFlowEdge {
    pub from: BlockId,
    pub to: BlockId,
}

/// Builder for constructing a CFG.
pub struct CfgBuilder {
    blocks: Vec<BasicBlock>,
    edges: Vec<ControlFlowEdge>,
    next_block_id: u32,
    next_stmt_id: u32,
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
    builder.build_statements(db, func.body(db), entry_block);

    ControlFlowGraph::new(db, builder.blocks, builder.edges)
}

impl CfgBuilder {
    /// Create a new CFG builder.
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            edges: Vec::new(),
            next_block_id: 0,
            next_stmt_id: 0,
        }
    }

    /// Allocate a new block ID.
    fn alloc_block_id(&mut self) -> BlockId {
        let id = BlockId(self.next_block_id);
        self.next_block_id += 1;
        id
    }

    /// Allocate a new statement ID.
    fn alloc_stmt_id(&mut self) -> StmtId {
        let id = StmtId(self.next_stmt_id);
        self.next_stmt_id += 1;
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
    /// Returns the block ID where control flow exits (or None if all paths diverge).
    fn build_statements<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        stmts: &[Statement<'db>],
        start_block: BlockId,
    ) -> Option<BlockId> {
        let mut current_block = start_block;
        let mut current_stmts = Vec::new();

        for stmt in stmts {
            match stmt {
                Statement::Let(let_stmt) => {
                    // Simple statement - add to current block.
                    let stmt_id = self.alloc_stmt_id();
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
                        self.add_block(BasicBlock {
                            block_id: current_block,
                            statements: current_stmts.clone(),
                            terminator: Terminator::Branch {
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

                Statement::Ret(ret_stmt) => {
                    // Return statement terminates the current block.
                    let stmt_id = self.alloc_stmt_id();
                    current_stmts.push(stmt_id);

                    self.add_block(BasicBlock {
                        block_id: current_block,
                        statements: current_stmts,
                        terminator: Terminator::Return,
                    });

                    // No continuation after return.
                    return None;
                }

                Statement::If(if_stmt) => {
                    // If-statement creates a branch.
                    let stmt_id = self.alloc_stmt_id();
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
                            then_block,
                            else_block,
                        },
                    });

                    // Add edges to then and else blocks.
                    self.add_edge(current_block, then_block);
                    self.add_edge(current_block, else_block);

                    // Build then branch.
                    let then_exit = self.build_statements(db, if_stmt.then_body(db), then_block);
                    if let Some(then_exit_block) = then_exit {
                        // Then branch doesn't return - add edge to join block.
                        self.add_edge(then_exit_block, join_block);
                        self.add_block(BasicBlock {
                            block_id: then_exit_block,
                            statements: Vec::new(),
                            terminator: Terminator::Goto(join_block),
                        });
                    }

                    // Build else branch (if present).
                    if let Some(else_body) = if_stmt.else_body(db) {
                        let else_exit = self.build_statements(db, else_body, else_block);
                        if let Some(else_exit_block) = else_exit {
                            // Else branch doesn't return - add edge to join block.
                            self.add_edge(else_exit_block, join_block);
                            self.add_block(BasicBlock {
                                block_id: else_exit_block,
                                statements: Vec::new(),
                                terminator: Terminator::Goto(join_block),
                            });
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
                    current_block = join_block;
                    current_stmts = Vec::new();
                }

                Statement::Fun(_) => {
                    // Nested functions not yet supported.
                }

                Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                    // No control flow impact.
                }
            }
        }

        // If we have remaining statements, create a final block.
        if !current_stmts.is_empty() {
            self.add_block(BasicBlock {
                block_id: current_block,
                statements: current_stmts,
                terminator: Terminator::Return, // Implicit return at end
            });
        }

        Some(current_block)
    }

    /// Check if an expression may return early (contains ? or !).
    fn expr_may_return_early<'db>(&self, db: &'db dyn crate::Db, expr: ExprFun<'db>) -> bool {
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
        let builder = CfgBuilder::new();
        assert_eq!(builder.blocks.len(), 0);
        assert_eq!(builder.edges.len(), 0);
        assert_eq!(builder.next_block_id, 0);
        assert_eq!(builder.next_stmt_id, 0);
    }

    #[test]
    fn test_block_id_allocation() {
        let mut builder = CfgBuilder::new();
        let id1 = builder.alloc_block_id();
        let id2 = builder.alloc_block_id();
        assert_eq!(id1.0, 0);
        assert_eq!(id2.0, 1);
    }

    #[test]
    fn test_stmt_id_allocation() {
        let mut builder = CfgBuilder::new();
        let id1 = builder.alloc_stmt_id();
        let id2 = builder.alloc_stmt_id();
        assert_eq!(id1.0, 0);
        assert_eq!(id2.0, 1);
    }

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
