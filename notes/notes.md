## Zipper Heaps

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

### Zipper heaps for pow-growable buffers like vec/list/string

You might not expect this to work for vector buffers
but I think it can if we enforce that they e.g. always allocate powers of two elements,
and those are our allocator buckets;
as vectors grow they maintain a linked list to previous buckets
that all have to be traversed during unzipping.

Zipper-heap candidates:

- bigint
- list
- string
- what about map and set?
- data and error

