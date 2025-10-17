Based on the BCT module and module_resolve,
and package2 and package_resolve2.

See for example how la2 (../la2) uses the bct module system.

here's what i want:

libraries contain packages and packages contain modules.

the "sys" library comes with the compiler,
the "local" library is for the user's workspace.
in the future there will be a library that represents the package ecosystem.

in this repo sys lives at `sys`
and the standard library at `sys/std`.
`std` contains `.dfm` datafun module files.

The compiler scans the sys directory for packages
and modules and loads them as bct needs.

Require syntax for modules looks like

```
require module sys/std/bool
require module sys/std/int
```

Always three parts - lib - pkg - module.
