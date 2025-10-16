# First Steps

Let's write your first Datalove program!

## Your First Datalit File

Create a file called `hello.dlt`:

```datalove
{
  greeting = "Hello, Datalove!",
  numbers = [1, 2, 3, 4, 5],
  is_awesome = @true,
}
```

This is a simple Datalit expression - a struct with three fields.

## Type Checking

Check that your file is valid:

```bash
datalove lit-tycheck hello.dlt
```

You should see: `No type errors found.`

## Pretty Printing

Format your Datalit code:

```bash
datalove lit-pretty hello.dlt
```

This will print a nicely formatted version of your code.

## Adding Type Hints

You can add explicit type hints to your Datalit expressions:

```datalove
: {
  greeting: string,
  numbers: [u32],
  is_awesome: bool,
} / {
  greeting = "Hello, Datalove!",
  numbers = [1, 2, 3, 4, 5],
  is_awesome = @true,
}
```

The `: <type> /` syntax separates the type hint from the expression.

## Using the REPL

Start an interactive REPL session:

```bash
datalove repl
```

Try entering expressions interactively:

```datalove
1 + 2
```

```datalove
[1, 2, 3]
```

Exit with `Ctrl+D` or `Ctrl+C`.

## Next Steps

- Learn more about [Datalit types](../reference/datalit/types.md)
- Explore [Datalit literals](../reference/datalit/literals.md)
- Read about [Type hints](../reference/datalit/type-hints.md)
- See more [examples](../reference/datalit/examples.md)
