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
Third, one of Datalove's [principles](../principles.md) is that performance characteristics
are reflected in the surface language.
We can't silently box the recursive instance.


In Rust we would write `struct Recursive(Box<Recursive>)`.

Here's a datalit struct that contains an empty struct field:

```datalove
{
  elt: int,
  next: ?{ } // what goes here?
}
```

If we were to solve this the obvious and easy way it might
create an alias and use a hypothetical `box` type:

```datalove
type List: {
  elt: int,
  next: ?box List
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
: type List: {
  elt: int,
  next: ?box List
} / {
  elt = 1,
  next = some box {
    elt = 2,
    next = none
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
: type List {
  elt: int,
  next: ?box List
} / {
  elt = 1,
  next = some box {
    elt = 2,
    next = none
  }
}
```

As a statement this weakens the policy of `:` always preceding types:

```datalove
type List {
  elt: int,
  next: box List
}
```

But maybe that can be rationalized.


## How can we surface the recursive boxing withot a box type?

Use `recursive` as a keyword to indicate
the cycle and as the word that heapifies the inner value.

```datalove
: type List {
  elt: int,
  next: ?recursive List
} / {
  elt = 1
  next = some recursive {
    elt = 2,
    next = none
  }
}
```

As statements:

```datalove
type List {
  elt: int,
  next: ?recursive List
}

let next: List = {
  elt = 2,
  next = none
}

let list: List = {
  elt = 1,
  next = some recursive next
}
```

Using `recursive` to mean "put this on the heap
and insert it into a slot representing a pointer
to a recursive instance"...
can this make sense with structural types?
We have to structurally verify
the outer type is a recursive type somehow.
Definitely needs a checking context.

Without an explicit `box` type that also
suggests that moving the inner field between
two nodes will unbox and rebox instead of
just moving the pointer.
Could maybe awkwardly work around that with memswap functions.

## Can we avoid the type alias?

```datalove
: {
  elt: int,
  next: ?recursive 1 // the index of the type-context stack
} / {
  elt = 1
  next = some recursive {
    elt = 2,
    next = none
  }
}
```

```datalove
: {
  elt: int,
  inner: {
    next: ?recursive 2 // 2 levels deep
  }
} / {
  elt = 1
  inner = {
    next = some recursive {
    elt = 2,
      inner = {
        next = none
      }
    }
  }
}
```
