<!--

Title: Recursive types in a declarative world of structural value types.
Summary:

ABTstract:
For types to contain themselves recursively
there needs to be a way to name them.
_But_ a structurally-typed language without type aliases
has no obvious way to name a type recursively.
For inline value types to contain themselves recursively
there needs to be a way to bound the inline size,
...
_But_ ...
_Therefore_ ...

-->

We have some fun constraints.
First, datalit is a declarative expression language with no facility for naming types.
Without naming a type we can't easily describe a recursive type.
Second, Datalove's algebraic types are value types,
stored inline at runtime.
They cannot have unbounded recursive size.
Third, one of Datalove's [principles](todo) is that performance characteristics
are reflected in the surface language.
We can't silently box the recursive instance.


In Rust we would write `struct Recursive(Box<Recursive>)`.

Here's a datalit struct that contains an empty struct field:

```datalove
{
  recursive: ?{ } // what goes here?
}
```

If we were to solve this the obvious and easy way it might
create an alias and use a hypothetical `box` type:

```datalove
type Recursive: {
  recursive: ?box Recursive
}
```

I don't want to do it this way,
neither the type aliases nor the boxing.

Although type aliases already exist in Datalove,
they do not exist in the datalit declarative language.
Type aliases are statements, and datalit is an expression language,
and I'd like to keep it that way.
_If_ I were to introduce type aliases to datalit I would want to
do it inline with the expression's type hint.

Although the explit `box` type is compatible with
Datalove's machine sympathy rule,
it would exist to provide a runtime semantic
that doesn't have any particular meaning to the _datalit_ declarative
serialazition lanugage.

Datalove has a constant tension imposed by its core design
wherein it defines its type system as a declarative serialization language,
and builds its imperative language off that.
I like this tension!
It keeps me from piling on features that can't be rationalized in both contexts.


## What would a type alias in a type hint look like?

```datalove
: type Recursive: {
  recursive: ?box Recursive
} / {
  recursive: some box {
    recursive: none
  }
}
```

(Note that Datalove does not have a way to name a type alias in constructor position.
It has to write the type structurally, not e.g. `Recursive { ... }`).

That looks parsable.
Obvious problem is the double colon.
Colon strongly means "what follows is a type" in Datalove.
In this combination of the existing type hint syntax and
type alias syntax it both introduces the type hint
and then immediately defines the type alias.
Could change the type alias syntax to fix this.

```datalove
: type Recursive {
  recursive: box Recursive
} / {
  recursive: some box {
    recursive: none
  }
}
```

As a statement this weakens the policy of `:` always preceding types:

```datalove
type Recursive {
  recursive: box Recursive
}
```


## How can we surface the recursive boxing withot a box type?

