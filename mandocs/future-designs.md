# Future designs





## 2026/01/21 - Ergonomic switches

- fixed int math op widening to int
- auto-@ operator

merge former into @.

Add "auto-coerce" feature,
off for modules,
on for scripts.

Could be toggleable:

```datalove
feature auto_coerce off
```

Comparison to visual basic modes that I've forgotten, js strict mode.

Makes intro scripting easy,
gives options when moving to writing modules.




### 2026-01-17 - `assert` statements

todo

```datalove
fun test_thing()

end fun
```




# Bitwise operators

Shift operators are logical.
For arithmetic right shift use divide by 2.

Can't have << and >> because of ambiguous lex.
Well we can have .<< and .>>.

bitand
bitor
bitnot
bitxor

.<< .>>
& | ~ ^

to use `|` we would need to change the `if expr |arg|` syntax.

