# Zipper Heaps

For datafun we should be able to track dataflow very precisely,
and there are are only a few types of allocations involved,
for which we have strong type info.

I believe we can create an algorithm that will use static analysis
of binding scopes to perfectly defragment the local heap
as scopes are crossed.

The exact details left to the imagination,
but it involves lots of buckets by type/size/alignment,
and swapping their backing-buffers and owned pointers when scopes cross
due to slot-clobbering, argument passing, or returning,
treating the heap more like a shadow stack.

## Zipper heaps for pow-growable buffers like vec/list/string

You might not expect this to work for vector buffers
but I think it can if we enforce that they e.g. always allocate powers of two elements,
and those are our allocator buckets;
as vectors grow they maintain a linked list (chain) to previous buckets
that all have to be traversed during unzipping,
which progressively "untangles" the zipper chains
algorithm unclear but seems doable

Zipper-heap candidates:

- bigint - can just grow in powers of two, potentially
- list and string - ditto
- map and set - more complex allocation patterns
- data and error - unclear but possible

As a simplification we are going to limit size-buckets to powers of 2 size
(or powers of 2 + some constant), but i don't see why it couldn't
support others.


## Strawman zipper-heap algorithm

The zipper heap is a type-directed allocator.
It requires close co-design with the language and type system.
The language must track all variable scopes
and instrument "unzip" calls wheven an inner scope crosses an outer scope -
*this process essentially "stackifies" the heap so that when an allocating
data type is destroyed all of their outstanding allocations,
some of which may be already dead and waiting to "GC"
are at the tip of their respective buckets and are simply popped*.

The unzip algorithm is the novelty and what is primarily
described here, needs to be fully designed and verified.

## Terms, preconditions, and requirements:

All allocations are by known containers with constrained allocation patterns.
Many possible uses of allocators would not be compatible.
All allocating Datalit types are containers that should be compatible.


