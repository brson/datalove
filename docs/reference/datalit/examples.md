# Datalit Examples

Complete examples of Datalit code.

## Personal Information

```datalove
{
  name = "Ada Lovelace",
  born = 1815,
  died = some(1852),
  interests = ["mathematics", "poetry", "music"],
  notable_work = "First computer program",
}
```

With type hints:

```datalove
: {
  name: string,
  born: u32,
  died: ?u32,
  interests: [string],
  notable_work: string,
} / {
  name = "Ada Lovelace",
  born = 1815,
  died = some(1852),
  interests = ["mathematics", "poetry", "music"],
  notable_work = "First computer program",
}
```

## Address Book

```datalove
set {
  struct Contact {
    name = "Alice",
    email = some("alice@example.com"),
    phone = none,
  },
  struct Contact {
    name = "Bob",
    email = none,
    phone = some("555-1234"),
  },
}
```

## Configuration File

```datalove
{
  app_name = "MyApp",
  version = (major = 1, minor = 0, patch = 0),
  features = map {
    "logging" => @true,
    "debug" => @false,
    "metrics" => @true,
  },
  database = {
    host = "localhost",
    port = 5432,
    name = "myapp_db",
  },
}
```

## Computation Results

```datalove
{
  status = enum Status::Success,
  result = ok(42),
  metadata = {
    duration_ms = 123,
    memory_used = 1024,
  },
}
```

## See Also

- [Types](types.md)
- [Literals](literals.md)
- [Type Hints](type-hints.md)
