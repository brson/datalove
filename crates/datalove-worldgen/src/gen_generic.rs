//! Generic function generation.
//!
//! A generic body can do very little with a value of its type parameter: move
//! it, drop it, clone it, print it, hand it on, or put it in a collection.
//! Anything else wants a bound, and a bound admits only some types. So rather
//! than teaching the expression generator what a `T` is and hoping it keeps to
//! those rules, the bodies here come from a fixed set of shapes that are known
//! to compile, and the variety is put where it is worth having: in the types
//! the call sites pick.
//!
//! That is also where the interesting work happens. A generic is compiled once
//! with `data` where its parameter was written, and each call converts on the
//! way in and back out, hands over a descriptor for anything the body builds,
//! and drops what it is left holding. None of that depends on the body being
//! elaborate; it depends on the type the call chose.

use rand::Rng;

use datalove_datalit::ast::{
    TypeHint, TypeHintAnonTuple, TypeHintList, TypeHintMap, TypeHintOption, TypeHintSet,
};

use crate::config::WorldGenConfig;
use crate::context::GenContext;
use crate::gen_expr::gen_expr;
use crate::gen_type::gen_type_hint;
use crate::pretty::pretty_type_hint;

/// What a type parameter has to promise before a shape may use it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    /// Nothing promised, which is every type.
    None,
    /// Ordered, which a set's elements and a map's keys have to be.
    Ord,
    /// One of the ten fixed-width integers.
    FixedInt,
    /// `f32` or `f64`.
    Float,
}

impl Bound {
    fn clause(&self, param: &str) -> String {
        match self {
            Bound::None => String::new(),
            Bound::Ord => format!(" with {{ {} is ord, }}", param),
            Bound::FixedInt => format!(" with {{ {} is fixedint, }}", param),
            Bound::Float => format!(" with {{ {} is float, }}", param),
        }
    }

    /// A type this bound admits.
    fn pick<'db, R: Rng>(
        &self,
        db: &'db dyn salsa::Database,
        rng: &mut R,
        config: &WorldGenConfig,
    ) -> TypeHint<'db> {
        match self {
            // Every type is ordered, so neither of these narrows anything.
            Bound::None | Bound::Ord => gen_type_hint(db, rng, config),
            Bound::FixedInt => match rng.gen_range(0..10) {
                0 => TypeHint::U8,
                1 => TypeHint::I8,
                2 => TypeHint::U16,
                3 => TypeHint::I16,
                4 => TypeHint::U32,
                5 => TypeHint::I32,
                6 => TypeHint::U64,
                7 => TypeHint::I64,
                8 => TypeHint::Index,
                _ => TypeHint::Offset,
            },
            Bound::Float => {
                if rng.gen_bool(0.5) { TypeHint::F32 } else { TypeHint::F64 }
            }
        }
    }
}

/// One of the generic functions this knows how to write.
///
/// Each says what it does with the value it is given, which is the whole of
/// what distinguishes them: handing it back, letting it go, cloning it,
/// wrapping it, or building a collection of it. The last are the ones that
/// make the call site hand over a descriptor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// `fun f<T>(x: T): T` -- moves the value straight through.
    Identity,
    /// `fun f<T>(x: T)` -- takes it and lets it go.
    Consume,
    /// `fun f<T>(x: T): (T, T)` -- clones, so the value is used twice.
    Duplicate,
    /// `fun f<T>(x: T): ?T` -- puts it somewhere that holds its payload
    /// inline, which erasure converts field by field.
    Wrap,
    /// `fun f<T>(x: T): [T]` -- builds a collection of a type it cannot name,
    /// which the call site has to hand a descriptor for.
    Listify,
    /// `fun f<T>(a: T, b: T): [T]` -- the same with two elements.
    ListifyTwo,
    /// `fun f<T>(x: T): #{T}` -- a set, whose elements are kept in order.
    Setify,
    /// `fun f<K, V>(k: K, v: V): %{K = V}` -- two parameters, one of them
    /// ordered and the other not.
    Mapify,
    /// `fun f<T>(ref x: [T]): bool` -- borrows a collection of a parameter,
    /// where nothing is converted and the descriptor comes from the argument.
    Borrow,
    /// `fun f<T>(a: T, b: T): ?T with { T is fixedint, }` -- checked
    /// arithmetic on a bounded parameter.
    AddChecked,
    /// `fun f<T>(a: T, b: T): T with { T is float, }` -- bare arithmetic,
    /// which only a float has.
    Multiply,
    /// `fun f<T>(a: T, b: T): bool with { T is fixedint, }` -- a comparison,
    /// whose answer is not the parameter's type. One shape per operator: a
    /// generic body is where most comparisons in the corpus are written, and
    /// with `.<` alone the other five were left to the handful of places that
    /// write a condition, thin enough to reach zero between runs.
    Compare(Comparison),
}

/// The six ways two values of an ordered type are compared.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comparison {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

impl Comparison {
    fn written(&self) -> &'static str {
        match self {
            Comparison::Lt => ".<",
            Comparison::Gt => ".>",
            Comparison::Le => "<=",
            Comparison::Ge => ">=",
            Comparison::Eq => "==",
            Comparison::Ne => "!=",
        }
    }
}

impl Shape {
    /// Every shape, for picking one at random.
    pub fn all() -> &'static [Shape] {
        &[
            Shape::Identity, Shape::Consume, Shape::Duplicate, Shape::Wrap,
            Shape::Listify, Shape::ListifyTwo, Shape::Setify, Shape::Mapify,
            Shape::Borrow, Shape::AddChecked, Shape::Multiply,
            Shape::Compare(Comparison::Lt),
            Shape::Compare(Comparison::Gt),
            Shape::Compare(Comparison::Le),
            Shape::Compare(Comparison::Ge),
            Shape::Compare(Comparison::Eq),
            Shape::Compare(Comparison::Ne),
        ]
    }

    /// The type parameters it declares, and what each must promise.
    fn params(&self) -> &'static [(&'static str, Bound)] {
        match self {
            Shape::Setify => &[("T", Bound::Ord)],
            Shape::Mapify => &[("K", Bound::Ord), ("V", Bound::None)],
            Shape::AddChecked | Shape::Compare(_) => &[("T", Bound::FixedInt)],
            Shape::Multiply => &[("T", Bound::Float)],
            _ => &[("T", Bound::None)],
        }
    }
}

/// A generic function that has been written, and what a call to it needs.
#[derive(Clone)]
pub struct GenericSig {
    pub name: String,
    pub shape: Shape,
}

impl GenericSig {
    /// The type parameters and their bounds, in declaration order.
    pub fn type_params(&self) -> &'static [(&'static str, Bound)] {
        self.shape.params()
    }
}

/// Write the definition of a generic function.
pub fn gen_generic_function(name: &str, shape: Shape) -> String {
    let params = shape.params();
    let names = params.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    // Only one parameter of a shape ever carries a bound, and it is the first.
    let clause = params[0].1.clause(params[0].0);

    match shape {
        Shape::Identity => format!(
            "fun {name}<{names}>(x: T): T{clause}\n  ret x\nend fun"),
        Shape::Consume => format!(
            "fun {name}<{names}>(x: T){clause}\n  let unwanted: T = x\nend fun"),
        Shape::Duplicate => format!(
            "fun {name}<{names}>(x: T): (T, T){clause}\n  ret (x@, x)\nend fun"),
        Shape::Wrap => format!(
            "fun {name}<{names}>(x: T): ?T{clause}\n  ret some x\nend fun"),
        Shape::Listify => format!(
            "fun {name}<{names}>(x: T): [T]{clause}\n  ret [x]\nend fun"),
        Shape::ListifyTwo => format!(
            "fun {name}<{names}>(a: T, b: T): [T]{clause}\n  ret [a, b]\nend fun"),
        Shape::Setify => format!(
            "fun {name}<{names}>(x: T): #{{T}}{clause}\n  ret #{{x}}\nend fun"),
        Shape::Mapify => format!(
            "fun {name}<{names}>(k: K, v: V): %{{K = V}}{clause}\n  ret %{{k = v}}\nend fun"),
        Shape::Borrow => format!(
            "fun {name}<{names}>(ref x: [T]): bool{clause}\n  ret true\nend fun"),
        Shape::AddChecked => format!(
            "fun {name}<{names}>(a: T, b: T): ?T{clause}\n  ret some (a +? b)\nend fun"),
        Shape::Multiply => format!(
            "fun {name}<{names}>(a: T, b: T): T{clause}\n  ret a * b\nend fun"),
        Shape::Compare(how) => format!(
            "fun {name}<{names}>(a: T, b: T): bool{clause}\n  ret a {} b\nend fun",
            how.written()),
    }
}

/// Write a call to a generic function, and say what type the answer has.
///
/// The types are chosen here rather than inferred. A generic's parameter is
/// fixed by what the arguments turn out to be, so an argument that does not
/// say its own type would fix it somewhere else: an unadorned `1.5` is an
/// `f64`, and a call meant for `f32` would quietly become a call for `f64`
/// and then disagree with the binding that named it.
///
/// So every argument is bound to a name carrying its type first, and the call
/// is written over the names. That also gives a `ref` parameter something to
/// borrow, which an expression is not.
///
/// Returns the lines to put before the call, the call itself, and the type of
/// what comes back -- `None` when nothing does, which makes the call a
/// statement on its own.
pub fn gen_generic_call<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    sig: &GenericSig,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
) -> (Vec<String>, String, Option<TypeHint<'db>>) {
    let chosen: Vec<TypeHint<'db>> = sig
        .type_params()
        .iter()
        .map(|(_, bound)| bound.pick(db, rng, config))
        .collect();
    let t = chosen[0].clone();

    let list_of = |ty: TypeHint<'db>| TypeHint::List(TypeHintList { element_type: Box::new(ty) });

    let mut prelude: Vec<String> = Vec::new();
    let mut bind = |rng: &mut R,
                    ctx: &mut GenContext<'db>,
                    prelude: &mut Vec<String>,
                    ty: TypeHint<'db>| -> String {
        let name = format!("a{}", *var_counter);
        *var_counter += 1;
        let value = gen_expr(db, rng, ty.clone(), config, ctx);
        prelude.push(format!("let {}: {} = {}", name, pretty_type_hint(db, ty), value));
        name
    };

    let (call, result) = match sig.shape {
        Shape::Identity => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            (format!("{}({})", sig.name, a), Some(t))
        }
        Shape::Consume => {
            let a = bind(rng, ctx, &mut prelude, t);
            (format!("{}({})", sig.name, a), None)
        }
        Shape::Duplicate => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let pair = TypeHint::AnonTuple(TypeHintAnonTuple { fields: vec![t.clone(), t] });
            (format!("{}({})", sig.name, a), Some(pair))
        }
        Shape::Wrap => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let opt = TypeHint::Option(TypeHintOption { inner_type: Box::new(t) });
            (format!("{}({})", sig.name, a), Some(opt))
        }
        Shape::Listify => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            (format!("{}({})", sig.name, a), Some(list_of(t)))
        }
        Shape::ListifyTwo => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let b = bind(rng, ctx, &mut prelude, t.clone());
            (format!("{}({}, {})", sig.name, a, b), Some(list_of(t)))
        }
        Shape::Setify => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let set = TypeHint::Set(TypeHintSet { element_type: Box::new(t) });
            (format!("{}({})", sig.name, a), Some(set))
        }
        Shape::Mapify => {
            let v = chosen[1].clone();
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let b = bind(rng, ctx, &mut prelude, v.clone());
            let map = TypeHint::Map(TypeHintMap {
                key_type: Box::new(t),
                value_type: Box::new(v),
            });
            (format!("{}({}, {})", sig.name, a, b), Some(map))
        }
        Shape::Borrow => {
            // Borrowed, so the argument has to be a place rather than an
            // expression, which is what binding it gives.
            let a = bind(rng, ctx, &mut prelude, list_of(t));
            (format!("{}(ref {})", sig.name, a), Some(TypeHint::Bool))
        }
        Shape::AddChecked => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let b = bind(rng, ctx, &mut prelude, t.clone());
            let opt = TypeHint::Option(TypeHintOption { inner_type: Box::new(t) });
            (format!("{}({}, {})", sig.name, a, b), Some(opt))
        }
        Shape::Multiply => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let b = bind(rng, ctx, &mut prelude, t.clone());
            (format!("{}({}, {})", sig.name, a, b), Some(t))
        }
        Shape::Compare(_) => {
            let a = bind(rng, ctx, &mut prelude, t.clone());
            let b = bind(rng, ctx, &mut prelude, t);
            (format!("{}({}, {})", sig.name, a, b), Some(TypeHint::Bool))
        }
    };

    (prelude, call, result)
}
