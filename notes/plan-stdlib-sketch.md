# Project: see how much datalove std we can write

We have a standard library in sys/std,
and a test suite for it in std_tests.

We have recently improved the interpreter
such that most of the features in the botspec are supported.

lets do this:

reevaluate the state of std and std_tests:
does the existing implementation make sense and does it have thorough test coverage?

then lets consider filling out the existing std modules
with features that can be built off the existing language
features -
that is also without adding any runtime calls from std,
for which we don't yet have a mechanism.

make a list of potential functions to add to existing std modules.
mostly consider rust's core library in comparison.

make a list of potential basic core modules and their functions to add to std
to support the current datalove featureset