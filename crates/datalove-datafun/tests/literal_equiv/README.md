# Kept literal equivalence cases

`literal_equiv_tests` runs every `.case` here before any seed, and each must
come out the same in datalit and datafun. A case is what a difference the
suite finds is written out as: a `mode:` (`inferred`, `valued`, `checked`,
`argument`, `return`, `set`, `var` or `mutated`), a `type:` for the modes
that check against one, and an `expr:`, among `//` comments that say why it
is kept.

A difference found and fixed is kept here, copied from where the suite wrote
it, with its comments rewritten to say what was wrong. So are cases written by
hand for a rule the two languages state differently, which generated
expressions reach seldom.
