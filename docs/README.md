# Datalove Documentation

This directory contains the official Datalove documentation written in markdown.

## Building the Docs

Generate the static HTML documentation site:

```bash
datalove docs build
```

Output will be in `target/www/docs/` by default.

## Documentation Structure

- `index.md` - Main landing page
- `getting-started/` - Installation and first steps
- `reference/` - Language reference
  - `datalit/` - Datalit (data language)
  - `datafun/` - Datafun (function language)
  - `datalove/` - Full Datalove language
- `guides/` - In-depth guides and tutorials
- `cli/` - CLI command reference
- `design/` - Design philosophy and influences

## Writing Documentation

Documentation is written in GitHub-flavored markdown with these conventions:

### Code Blocks

Use the `datalove` language tag for syntax highlighting:

````markdown
```datalove
{
  name = "Ada",
  age = 36,
}
```
````

### Links

Link to other docs using relative paths:

```markdown
See [Types](types.md) for more information.
```

Links are automatically converted to `.html` during the build process.

### Headers

Use descriptive headers with proper hierarchy:

```markdown
# Page Title (H1)

## Major Section (H2)

### Subsection (H3)
```

## Reading the Docs

The raw markdown files are designed to be readable directly from the filesystem or GitHub. You don't need to build the HTML to read the documentation.

## Contributing

When adding new documentation:

1. Create markdown files in the appropriate directory
2. Add links from related pages
3. Build and preview the docs
4. Check that all links work correctly

## Building Options

Custom source and output directories:

```bash
datalove docs build --source my-docs --output my-output
```
