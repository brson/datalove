//! The codes of the datalove compiler's diagnostics.
//!
//! Each family of codes is an enum whose discriminant is the code's number,
//! so two faults given one number is a compile error (E0081), and a site that
//! reports a fault names it: `PCode::ForWithoutRef`, not `"P088"`. A site
//! that reuses a code for a different fault then reads as wrong.
//!
//! - [`DCode`]: ownership faults (D001 to D016), and datalit's parse faults
//!   (D017 on), which datafun's parser also reports for the literal syntax it
//!   shares with datalit.
//! - [`PCode`]: datafun's own parse faults.
//! - [`TCode`]: datalit's type faults.
//! - [`FCode`]: datafun's type faults.
//!
//! A new fault takes the next number after the family's highest. A number
//! no longer reported goes in the family's `RETIRED` list rather than being
//! reused, since old output and old docs still say what it meant.
//!
//! The tests hold the enums to the tables in `botdocs/`, and hold the rest of
//! the workspace to writing no code as a string literal.

use std::fmt;

/// A diagnostic code of any family.
pub trait Code: Copy + fmt::Debug {
    /// The code as it is shown, such as `"P088"`.
    fn as_str(self) -> &'static str;
}

/// The four bytes of a code's text.
#[doc(hidden)]
pub const fn code_bytes(prefix: u8, number: u16) -> [u8; 4] {
    assert!(number < 1000);
    [
        prefix,
        b'0' + (number / 100) as u8,
        b'0' + (number / 10 % 10) as u8,
        b'0' + (number % 10) as u8,
    ]
}

/// A code's text as a `str`.
#[doc(hidden)]
pub const fn code_str(bytes: &'static [u8; 4]) -> &'static str {
    match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => panic!("a code is ASCII"),
    }
}

macro_rules! codes {
    (
        $(#[$meta:meta])*
        pub enum $name:ident = $prefix:literal {
            $( $(#[$vmeta:meta])* $variant:ident = $number:literal, )*
        }
        retired = [$($retired:literal),* $(,)?];
    ) => {
        $(#[$meta])*
        #[repr(u16)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, enum_iterator::Sequence)]
        pub enum $name {
            $( $(#[$vmeta])* $variant = $number, )*
        }

        impl $name {
            /// The letter every code of the family begins with.
            pub const PREFIX: char = $prefix as char;

            /// Numbers once reported and not to be given out again.
            pub const RETIRED: &'static [u16] = &[$($retired),*];

            /// The code as it is shown.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => {
                            const BYTES: [u8; 4] = code_bytes($prefix, $number);
                            const TEXT: &str = code_str(&BYTES);
                            TEXT
                        }
                    )*
                }
            }

            /// The code's number.
            pub const fn number(self) -> u16 {
                self as u16
            }
        }

        impl Code for $name {
            fn as_str(self) -> &'static str {
                $name::as_str(self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

codes! {
    /// Ownership faults, and the parse faults of datalit's syntax.
    ///
    /// Tabled in `botdocs/botspec.md` (11.1) up to D016, and in
    /// `botdocs/datalit-ebnf.md` (Diagnostics) from D017.
    pub enum DCode = b'D' {
        /// Using a value after it was moved.
        UseAfterMove = 1,
        /// Moving a value twice.
        DoubleMove = 2,
        /// Moving a `ref`, `mut` or `out` parameter.
        CannotMoveBorrowed = 3,
        /// Passing `ref` where `mut` is required.
        CannotMutFromRef = 4,
        /// Reading an `out` parameter or `var` binding before it is set.
        ReadUninitialized = 5,
        /// Returning without initializing an `out` parameter.
        OutParamNotInitialized = 6,
        /// Moving an outer-scoped value in a loop.
        MoveInLoop = 7,
        /// Branches disagree on whether a value is held.
        InconsistentBranchMove = 8,
        /// Writing fields of an `out` parameter one at a time.
        OutParamPartialWrite = 9,
        /// Two arguments share a place root, one of them `mut` or `out`.
        AliasedMutableArgument = 10,
        /// An immutable binding passed as `mut` or `out`.
        CannotMutateImmutable = 11,
        /// A temporary passed as `mut` or `out`.
        CannotMutateTemporary = 12,
        /// Using a binding an earlier script unit gave away.
        UseAfterMoveInEarlierUnit = 13,
        /// A value moved on one way out of a loop and not another.
        InconsistentLoopExit = 14,
        /// Changing the collection a `for` walks.
        WalkedCollectionChanged = 15,
        /// Moving out of a const or const parameter without `@`.
        CannotMoveConst = 16,
        /// A map entry without `=`.
        MapEntryWithoutEquals = 17,
        /// End of input or a bracket where an expression is expected.
        ExpectedExpression = 18,
        /// A malformed number.
        MalformedNumber = 19,
        /// An unexpected sigil or error token.
        UnexpectedToken = 20,
        /// Tokens left after an expression.
        TrailingTokens = 21,
        /// Tokens left inside a type's brackets.
        TrailingTypeTokens = 22,
        /// Tensor groups of different shapes.
        TensorShapeMismatch = 23,
        /// A table row with the wrong number of cells.
        TableRowWidth = 30,
        /// A table header cell that is not a name.
        TableHeaderNotName = 31,
        /// An `enum` type without `{`.
        EnumTypeWithoutBrace = 32,
        /// A `,` with nothing before it, in a table or tensor.
        EmptyBeforeComma = 33,
        /// An extra token in a table header cell.
        TableHeaderExtraToken = 34,
        /// `atom` with no name.
        AtomWithoutName = 35,
        /// `term` with no name.
        TermWithoutName = 36,
        /// An `enum` literal without `{`.
        EnumLiteralWithoutBrace = 37,
        /// A duplicate field, column, variant or binding name.
        DuplicateName = 38,
        /// A bad string escape.
        BadStringEscape = 39,
        /// A bad or empty tensor header.
        BadTensorHeader = 40,
        /// A tensor body that does not fit its header.
        TensorBodyMismatch = 41,
        /// A tensor type of rank 0.
        TensorTypeRankZero = 42,
        /// A non-ASCII name.
        NonAsciiName = 43,
        /// A map type without `=`.
        MapTypeWithoutEquals = 44,
        /// A bad tensor rank.
        BadTensorRank = 45,
        /// An unexpected token in a type.
        UnexpectedTypeToken = 46,
        /// A tensor type without its `,`.
        TensorTypeWithoutComma = 47,
        /// A struct or table type field without its name or `:`.
        StructTypeFieldSyntax = 48,
        /// A hint without its `/`.
        HintWithoutSlash = 49,
        /// A `-` not glued to a decimal number, or a signed hex literal.
        BadSign = 50,
        /// An atom type with no name.
        AtomTypeWithoutName = 51,
        /// A term type with no name.
        TermTypeWithoutName = 52,
        /// An enum variant that is not `atom` or `term`.
        BadEnumVariant = 53,
        /// A close bracket with no open.
        UnmatchedClose = 54,
        /// An open bracket never closed.
        UnclosedOpen = 55,
        /// A `rec` type with no name.
        RecWithoutName = 56,
        /// A `rec` type named with a word that is a type, or starts one.
        RecNamedAsType = 57,
        /// A struct field without its name or `=`.
        StructFieldSyntax = 58,
        /// A bare identifier where a literal is expected.
        BareIdentifier = 59,
        /// A number with a suffix.
        NumberSuffix = 60,
    }
    retired = [25];
}

codes! {
    /// Datafun's parse faults.
    ///
    /// Tabled in `botdocs/datafun-ebnf.md` (Diagnostics).
    pub enum PCode = b'P' {
        /// A line that begins no statement.
        UnknownStatement = 1,
        /// `fun` without its parameter list.
        FunWithoutParams = 2,
        /// A `require` path without the `/` after its import space.
        RequireWithoutSpaceSlash = 3,
        /// A `require` path without the `/` after its package.
        RequireWithoutPackageSlash = 4,
        /// `require` not followed by `module`, `data` or `rider`.
        BadRequireKind = 5,
        /// `import` without the `.` after its module.
        ImportWithoutDot = 6,
        /// `let` with no name or pattern.
        LetWithoutName = 7,
        /// `var` with no name or pattern.
        VarWithoutName = 8,
        /// `set` with no name.
        SetWithoutName = 9,
        /// `fun` with no name.
        FunWithoutName = 10,
        /// A parameter with no name.
        ParamWithoutName = 11,
        /// A parameter without its `:`.
        ParamWithoutColon = 12,
        /// `require module` with no import space.
        RequireModuleWithoutSpace = 13,
        /// A `require` path with no package.
        RequireWithoutPackage = 14,
        /// A `require` path with no module or data name.
        RequireWithoutName = 15,
        /// `require data` with no import space.
        RequireDataWithoutSpace = 16,
        /// `import` with no module.
        ImportWithoutModule = 17,
        /// `import` with no item.
        ImportWithoutItem = 18,
        /// A `|` binding with no name.
        PipeBindingWithoutName = 19,
        /// A `|` binding without its closing `|`.
        PipeBindingUnclosed = 20,
        /// A token left over after a statement or a bracket's last element,
        /// or a postfix operator written apart from what it applies to.
        StrayToken = 21,
        /// `type` with no name.
        TypeAliasWithoutName = 22,
        /// `type` without its `:`.
        TypeAliasWithoutColon = 23,
        /// `const` with no name.
        ConstWithoutName = 24,
        /// `const` without its `=`.
        ConstWithoutEquals = 25,
        /// A `var` with neither a type nor a value.
        VarWithoutTypeOrValue = 26,
        /// `require rider` with no name.
        RequireRiderWithoutName = 27,
        /// `intrinsic` without `native`, or `native` without `fun`.
        NativeFunHeader = 28,
        /// `native fun` with no name.
        NativeFunWithoutName = 29,
        /// `native fun` without its parameter list.
        NativeFunWithoutParams = 30,
        /// `call` of something that is not a call.
        CallOfNonCall = 31,
        /// Tokens after `end` and its keyword.
        EndLineExtraTokens = 33,
        /// A block with no `end` line.
        MissingEnd = 34,
        /// `icall` with no name.
        IcallWithoutName = 40,
        /// `icall` without its `(`.
        IcallWithoutParen = 41,
        /// `atom` with no name.
        AtomWithoutName = 42,
        /// `term` with no name.
        TermWithoutName = 43,
        /// `case atom` with no name.
        CaseAtomWithoutName = 44,
        /// `case term` with no name.
        CaseTermWithoutName = 45,
        /// `case term` with no binding.
        CaseTermWithoutBinding = 46,
        /// `case` not followed by `atom`, `term` or `default`.
        BadCaseKind = 47,
        /// A line before the first `case` of a `match`.
        LineOutsideCase = 48,
        /// A second `case default`.
        DuplicateDefaultCase = 49,
        /// A close bracket with no open.
        UnmatchedClose = 50,
        /// An open bracket never closed.
        UnclosedOpen = 51,
        /// A malformed number.
        MalformedNumber = 52,
        /// A number with a suffix.
        NumberSuffix = 53,
        /// A bound the language does not have.
        UnknownBound = 60,
        /// `where` without its `{`.
        WhereWithoutBrace = 61,
        /// A bound on a name that is not a type parameter of the function.
        BoundOnUndeclaredParam = 62,
        /// A bound without its `is`.
        BoundWithoutIs = 63,
        /// A reserved word declared as a name.
        ReservedName = 64,
        /// A binary operator straight after a constructor's payload.
        OperatorAfterPayload = 65,
        /// `else |e|` followed by `if`.
        ElseBindingThenIf = 66,
        /// A second comparison at the same level.
        ChainedComparison = 67,
        /// A destructuring `var` with no value.
        DestructuringVarWithoutValue = 68,
        /// A nested pattern, or a pattern without a name.
        NestedPattern = 69,
        /// A parenthesized name as a pattern.
        ParenthesizedName = 70,
        /// An `enum` literal without `{`.
        EnumLiteralWithoutBrace = 71,
        /// A call of something that is not a function.
        CallOfNonFunction = 72,
        /// A binding `if` in an `else if` chain.
        DestructuringElseIf = 73,
        /// A declaration inside a block.
        DeclarationInBlock = 74,
        /// A non-ASCII name.
        NonAsciiName = 75,
        /// `inline` anywhere but the whole first line of a function body.
        MisplacedInline = 76,
        /// A statement at the top level of a module that is not a declaration.
        StatementInModule = 77,
        /// A type parameter bounded twice.
        TypeParamBoundedTwice = 78,
        /// A malformed field selector after `.`.
        BadFieldSelector = 79,
        /// An unexpected token in an expression.
        UnexpectedToken = 80,
        /// A bracket that starts no expression, or no expression at all.
        ExpectedExpression = 81,
        /// A const parameter with a mode.
        ConstParamWithMode = 82,
        /// `let` without its `=`.
        LetWithoutEquals = 83,
        /// Anything but `native fun` and `type` in a rider interface.
        NotInRiderInterface = 84,
        /// An `intrinsic native` the compiler does not know.
        UnknownIntrinsic = 85,
        /// A `for` that binds no name, or more than two.
        ForNames = 86,
        /// A `for` without `in`.
        ForWithoutIn = 87,
        /// A `for` without `ref` before its collection.
        ForWithoutRef = 88,
        /// A `set` without its assignment operator.
        SetWithoutAssignOp = 89,
    }
    retired = [];
}

codes! {
    /// Datalit's type faults.
    ///
    /// Tabled in `botdocs/datalit-typing-rules.md` (Diagnostics).
    pub enum TCode = b'T' {
        /// A literal out of range for `u8`.
        U8OutOfRange = 5,
        /// A literal out of range for `i8`.
        I8OutOfRange = 6,
        /// A literal out of range for `u16`.
        U16OutOfRange = 7,
        /// A literal out of range for `i16`.
        I16OutOfRange = 8,
        /// A literal out of range for `u32`.
        U32OutOfRange = 9,
        /// A literal out of range for `i32`.
        I32OutOfRange = 10,
        /// A literal out of range for `u64`.
        U64OutOfRange = 11,
        /// A literal out of range for `i64`.
        I64OutOfRange = 12,
        /// A hex literal too wide for an `f32` bit pattern.
        F32BitsOutOfRange = 13,
        /// A hex literal too wide for an `f64` bit pattern.
        F64BitsOutOfRange = 14,
        /// A literal out of range for `index`.
        IndexOutOfRange = 15,
        /// A type left open with nothing to decide it.
        OpenType = 16,
        /// An expression that contains a parse error.
        ParseErrorInExpr = 17,
        /// `true`, `false` or a string literal against another type.
        LiteralMismatch = 22,
        /// `Check-Subsume` failed, or `none` or `er` against a type it is no
        /// value of.
        Mismatch = 32,
        /// A tuple length mismatch.
        TupleLength = 38,
        /// A struct field count mismatch.
        StructFieldCount = 39,
        /// A hint that differs from the expected type.
        HintMismatch = 40,
        /// A struct field out of order or misnamed.
        StructFieldName = 42,
        /// A tensor rank mismatch.
        TensorRank = 48,
        /// A tensor element count that does not match the shape.
        TensorElementCount = 49,
        /// A table column count mismatch.
        TableColumnCount = 54,
        /// A table column name mismatch.
        TableColumnName = 55,
        /// A table row with the wrong number of elements.
        TableRowWidth = 56,
        /// An atom or term that is not the expected one.
        VariantMismatch = 57,
        /// An enum literal against a type that is not an enum.
        EnumLiteralNotEnum = 58,
        /// A bare name in a hint.
        NamedType = 59,
        /// A literal out of range for `offset`.
        OffsetOutOfRange = 61,
        /// A `rec` that never uses its name.
        RecUnused = 62,
        /// A `rec` named as an enclosing `rec` is.
        RecShadowed = 63,
        /// A `rec`'s name used with no collection between it and its `rec`.
        RecUnguarded = 64,
        /// A recursive type in a map's key or a set's element.
        RecInKey = 65,
        /// A type parameter inside a recursive type.
        RecGeneric = 66,
    }
    retired = [18, 19, 20, 21, 51, 52, 60];
}

codes! {
    /// Datafun's type faults.
    ///
    /// Tabled in `botdocs/datafun-typing-rules.md` (Diagnostics).
    pub enum FCode = b'F' {
        /// An undefined variable.
        UndefinedVariable = 1,
        /// An undefined function.
        UndefinedFunction = 2,
        /// No synthesis rule applies.
        NoSynthesisRule = 11,
        /// A type mismatch.
        Mismatch = 16,
        /// An operator applied to a type it does not admit.
        InvalidOperand = 26,
        /// The wrong number of arguments.
        ArgumentCount = 45,
        /// A binding `if` on a result with no `else` binding.
        ResultWithoutElse = 46,
        /// `?` on a non-option or `!` on a non-result.
        BadUnwrap = 48,
        /// An early return whose type does not fit, or outside any function.
        EarlyReturnMismatch = 49,
        /// `break` outside a loop.
        BreakOutsideLoop = 50,
        /// `continue` outside a loop.
        ContinueOutsideLoop = 51,
        /// `ret e` in a function with no declared return type.
        RetValueInVoid = 52,
        /// Bare `ret` in a function with a return type.
        BareRetInValued = 53,
        /// `set` on an undefined variable.
        SetUndefined = 54,
        /// `set` on an immutable binding.
        SetImmutable = 55,
        /// An argument mode marker that does not match the parameter's mode.
        ModeMismatch = 57,
        /// A const initializer naming something that is not a const.
        ConstNamesNonConst = 58,
        /// A name imported twice.
        ImportedTwice = 59,
        /// A type parameter in a set or map without `ord`.
        UnorderedParamInCollection = 61,
        /// `native fun` outside a rider.
        NativeOutsideRider = 62,
        /// A function with a return type whose body can complete.
        MissingReturn = 63,
        /// An unknown type name.
        UnknownType = 64,
        /// A literal out of range for its type.
        LiteralOutOfRange = 65,
        /// A type alias defined twice.
        AliasDefinedTwice = 66,
        /// No such field.
        NoSuchField = 67,
        /// A projection on a type with no fields.
        NoFields = 68,
        /// A tuple index out of range.
        TupleIndexOutOfRange = 69,
        /// A non-copy field read outside a borrowed position.
        NonCopyFieldRead = 70,
        /// A destructuring pattern that does not fit the value.
        PatternMismatch = 71,
        /// A non-copy element read outside a borrowed position.
        NonCopyElementRead = 72,
        /// A tensor view passed `mut` or `out`, or written by `set`.
        TensorViewMutated = 73,
        /// A bare index in a `set` target where one cannot go.
        BareIndexInSet = 74,
        /// A literal whose shape does not fit its type.
        LiteralShape = 75,
        /// An argument to a const parameter that is not a const binding.
        ConstArgNotConst = 76,
        /// A const parameter whose type mentions a type parameter.
        ConstParamGeneric = 77,
        /// An import or qualified call through a name no `require` introduced.
        NotRequired = 78,
        /// A `require` of something that does not exist.
        RequireNotFound = 79,
        /// The same module or rider required twice in one unit.
        RequiredTwice = 80,
        /// A module required under an alias another already has.
        AliasTaken = 81,
        /// `require rider` in a script.
        RiderInScript = 82,
        /// A `require` that closes a cycle of modules.
        RequireCycle = 83,
        /// A `require data` whose file does not have the given type.
        DataTypeMismatch = 84,
        /// `inline` functions calling each other in a cycle.
        InlineCycle = 85,
        /// A function defined twice, or imported and defined.
        FunctionDefinedTwice = 86,
        /// A `for` over something it cannot walk.
        BadFor = 87,
        /// A const whose type mentions a type parameter.
        ConstTypeGeneric = 88,
        /// A const that depends on itself or on a later const.
        ConstCycle = 89,
    }
    retired = [60];
}
