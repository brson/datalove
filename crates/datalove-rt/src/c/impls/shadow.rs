//! The shadow stack: storage for values whose size is known only at run time.
//!
//! Inside a generic, a value whose type mentions a type parameter has no size
//! the compiler knows, so it cannot sit in the function's frame. Its frame
//! slot holds a pointer and a descriptor instead, and the value itself lives
//! here. A function takes what it needs on entry and gives it back on exit,
//! so this is a stack, in step with the native one.
//!
//! The top and the limit of the space in use are in `RtHeader`, so that
//! compiled code can bump and restore them inline. It calls the runtime only
//! when a request does not fit (`dtlv_rti_shadow_alloc`).
//!
//! The space is a list of chunks, and growing moves to the next chunk rather
//! than moving what is already allocated, so a pointer into the stack stays
//! good for as long as what it points at is live. Because the current chunk
//! changes as the stack grows, the position a function restores on exit is
//! the top *and* the limit, both as they were on entry.
//!
//! Not every exit restores: compiled code abandoning its calls returns without
//! running its epilogue, and a panic unwinds straight past it. Both end in a
//! `reset`, which empties the stack along with everything else.

use datalove_rti::RtHeader;

/// The size of a chunk, unless a request needs a larger one.
const CHUNK_SIZE: usize = 64 * 1024;

/// The alignment of every chunk, and the most a request may ask for.
const CHUNK_ALIGN: usize = 16;

/// The chunks behind the shadow stack.
///
/// The chunk in use is the one whose end is the header's limit. Chunks after
/// it hold nothing live: the stack has unwound out of them.
pub struct ShadowStack {
    chunks: Vec<Chunk>,
}

struct Chunk {
    base: *mut u8,
    size: usize,
}

impl Chunk {
    fn new(size: usize) -> Chunk {
        let layout = std::alloc::Layout::from_size_align(size, CHUNK_ALIGN).expect("a shadow stack chunk's layout");
        let base = unsafe { std::alloc::alloc(layout) };
        if base.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Chunk { base, size }
    }

    fn end(&self) -> usize {
        self.base as usize + self.size
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        let layout = std::alloc::Layout::from_size_align(self.size, CHUNK_ALIGN).expect("a shadow stack chunk's layout");
        unsafe { std::alloc::dealloc(self.base, layout) };
    }
}

impl ShadowStack {
    /// A stack of one chunk, which `header`'s top and limit are set to.
    pub fn new(header: &mut RtHeader) -> ShadowStack {
        let stack = ShadowStack { chunks: vec![Chunk::new(CHUNK_SIZE)] };
        stack.empty(header);
        stack
    }

    /// Point `header` at the start of the first chunk, giving everything back.
    fn empty(&self, header: &mut RtHeader) {
        header.shadow_top = self.chunks[0].base as usize;
        header.shadow_limit = self.chunks[0].end();
    }

    /// Give everything back, and let go of every chunk but the first.
    pub fn reset(&mut self, header: &mut RtHeader) {
        self.chunks.truncate(1);
        self.empty(header);
    }

    /// Take `size` bytes aligned to `align`, moving to a new chunk if the
    /// current one has no room.
    pub fn alloc(&mut self, header: &mut RtHeader, size: usize, align: usize) -> *mut u8 {
        assert!(align.is_power_of_two() && align <= CHUNK_ALIGN, "a shadow stack request aligned to {align}");
        let start = align_up(header.shadow_top, align);
        if start + size <= header.shadow_limit {
            header.shadow_top = start + size;
            return start as *mut u8;
        }

        let current = self.chunks.iter().position(|c| c.end() == header.shadow_limit)
            .expect("the shadow stack's limit is the end of one of its chunks");
        // Everything after the current chunk is unused, so a chunk there that
        // is too small is replaced rather than skipped.
        let next = current + 1;
        if self.chunks.get(next).is_none_or(|c| c.size < size) {
            self.chunks.truncate(next);
            self.chunks.push(Chunk::new(CHUNK_SIZE.max(align_up(size, CHUNK_ALIGN))));
        }
        let chunk = &self.chunks[next];
        header.shadow_top = chunk.base as usize + size;
        header.shadow_limit = chunk.end();
        chunk.base
    }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}
