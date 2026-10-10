---
title: "Checkpoint 1"
category: dev
summary: "We've reached the first stop in the roadmap"
---

I've decided to close out [Checkpoint 1](../roadmap.md#user-content-checkpoint-1---compiler-architecture).
The last remaining item for some time was to write sufficient human docs.
The task is never over, and eventually one just has to call it done.
So it's done for now.

The human docs consist of:

- [Datalove Literals](../datalit.md) - an informal spec of the data model and declarative language.
- [Datalove Functions Primer](../datafun.md) - a short and kinda complete description of the functional-imperative language.
- [Datalove Principles](../principles.md) - guiding principles for the language.

Besides the docs, this checkpoint was primarily about
establishing a baseline language design and compiler architecture
that can uphold those principles
and serve as a foundation for my own personal language-design goals.

I'm feeling happy about it!


## Some of the things I like

The hard distinction between Datalove Literals and Datalove Functions.
Even if the declarative language isn't useful as a general-purpose serialization format
(and I don't think it is), I continue to find value in the declarative language.
I think it helps me clarify my thinking about the type system,
imposing hard rules about the syntactic representation of values and synthesizability of types;
and I think it will help learners understand the functions language by first understanding
the declarative language.

The acyclic module system requires all dependencies to be declared explicitly
in a way that is easy to scan, and fully resolves quickly and early.

The compiler has relatively few passes:
lex -> parse -> name resolution -> typecheck -> ownership -> IR lowering -> codegen.
So far they are relatively simple and fast.

Many backends are implemented:
the reference IR walker, bytecode, JIT via Cranelift, AOT via Cranelift, AOT via C.
The C backend is low-quality and exists to maintain a proof that it's possible.
There is no wasm backend, but the compiler builds for wasm,
I have done compile-to-wasm experiments, and I am confident the model is fully compatible.

The performance of the compiler, bytecode interpreter, and JIT are all satisfactory
and give me confidence in the language model.

The language fully supports compile-time function evaluation,
and the compiler is structured to phase it cleanly.

The syntax has no obvious ambiguities
and doesn't ever rely on lookahead to disambiguate potential conflicts.
It is a bit unwieldy and verbose in places.
For example, all statements lead with a reserved word.
I do like a simple old-school statement language.

The brace-matched lexing.
This is the first thing I wrote (myself!) for Datalove.
The way it allows line-oriented statement parsing without statement terminators,
but still allows statements to break across lines in natural ways, feels good to me.
I'm sure cases will come up where line-breaking is awkward,
but so far I'm happy with it.
I'm even fine that forcing `<` and `>` to be matched
sacrifices them as comparison operators, which become `.<` and `.>`.

```datalove
// No statement terminators anywhere.
fun foo<
  X, Y, Z
>(
  arg1: X, arg2: Y, arg3: Z
): {
  field1: X,
  field2: Y,
  field3: Z,
}
  let packed = (
    arg1, arg2, arg3
  )
  debuglog packed
  let (a, b, c) = packed
  ret {
    field1 = a,
    field2 = b,
    field3 = c,
  }
end fun
```

The analysis and execution model of chained script units is mostly
the same as for functions and function frames,
and seems like it's going to work well to fulfill my goals
of creating a reactive and deterministic interactive scripting environment.
Hard won, but nascent. Lots more to discover here.

The runtime architecture and type representations.
Also early code I wrote by hand.
The runtime is well-factored,
with a clear firewall between type representations,
the function interface,
and the implementation itself.
It doesn't use any global state or rely on platform-specific linker magic
and should be compatible with all reasonable target platforms.
Type representations are generally simple, what one would expect,
and suitable for forming a supportable public ABI.


## Some things I don't love

The heavy reliance on sigils for constructors of all the built-in types.
This is mostly to keep them unambiguous and to keep the declarative language concise.
There are more built-in types to come - am I going to keep finding new sigils for them?

I'm happy to have established very strict numeric semantics as a baseline,
but it is not very fun to do math in Datalove.
That's not great for a language explicitly intended for interactive use.
I expect to have to come up with a more ergonomic
experience that involves subtyping and/or lossless coercions.
Any concessions here will be done as a mode that can be activated or deactivated.

The `@` (adapt) operator.
So far it does just two things: clone, and perform lossless numeric conversions.
As a magic multi-purpose coercion tool, having just two tasks isn't pulling its weight.
And it's a syntactic eyesore.

Type-erased generics and their implementation.
I think monomorphization is not viable to achieve the compile-time performance I want,
but I am wary of the runtime overhead the current implementation adds to all operations,
the complexity of optimizing that,
and hard-to-understand performance cliffs.
I also find the current internal rules about how type descriptors are passed complex.

Generics also have only the most primitive form of trait bounds.
Enough to implement a few standard library functions.
Lots more to do here.
