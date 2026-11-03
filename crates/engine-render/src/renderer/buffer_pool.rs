//! Recycled vertex and index buffers for the meshes a frame builds and drops.
//!
//! A posed body, a live prop and a morph are rebuilt into a fresh mesh per
//! frame, the same size as last frame's. Creating two GPU buffers for each is the
//! expensive half of that (allocator, memory, the submit's tracking of them), so
//! a buffer dropped within [`TRANSIENT_FRAMES`] of its birth comes back here and
//! the next mesh of its exact size takes it and writes over it. A long-lived
//! mesh (a scene's statics, a memoised pose) frees its buffers as ever.
//!
//! A buffer returned in frame `F` is taken no earlier than frame `F + 1`: a
//! `write_buffer` runs ahead of the next submit, so a mesh dropped mid-frame must
//! not have its bytes replaced under commands already recorded against it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

/// A buffer dropped this many frames after its birth, or fewer, is recycled.
const TRANSIENT_FRAMES: u64 = 2;
/// A free buffer unclaimed this many frames is released to the driver.
const IDLE_FRAMES: u64 = 120;
/// Free bytes held at most: past this a returned buffer is simply dropped.
const MAX_FREE_BYTES: u64 = 256 << 20;

/// What a mesh buffer is bound as.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MeshBufferKind {
    Vertex,
    Index,
}

impl MeshBufferKind {
    fn usage(self) -> wgpu::BufferUsages {
        match self {
            Self::Vertex => wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            Self::Index => wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        }
    }
}

type Key = (u64, MeshBufferKind);

#[derive(Default)]
struct Inner {
    frame: u64,
    /// `(buffer, frame it came back)`, by exact size.
    free: HashMap<Key, Vec<(wgpu::Buffer, u64)>>,
    /// Returned this frame: not takeable until the frame's submit is past.
    returned: Vec<(Key, wgpu::Buffer)>,
    free_bytes: u64,
}

/// The pool a [`Renderer`](crate::Renderer) uploads through.
pub(crate) struct BufferPool {
    inner: Arc<Mutex<Inner>>,
}

impl BufferPool {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// Open a frame: last frame's returns become takeable, long-idle sizes go.
    pub(crate) fn next_frame(&self) {
        let Ok(mut g) = self.inner.lock() else { return };
        let inner = &mut *g;
        inner.frame += 1;
        let frame = inner.frame;
        for (key, buf) in inner.returned.drain(..) {
            inner.free.entry(key).or_default().push((buf, frame));
        }
        let mut released = 0;
        inner.free.retain(|key, list| {
            list.retain(|(_, back)| {
                let keep = frame - *back <= IDLE_FRAMES;
                if !keep {
                    released += key.0;
                }
                keep
            });
            !list.is_empty()
        });
        inner.free_bytes -= released;
    }

    /// A buffer holding `contents`: a free one of the exact size written over,
    /// or a new one.
    pub(crate) fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        kind: MeshBufferKind,
        contents: &[u8],
    ) -> PooledBuffer {
        use wgpu::util::DeviceExt as _;
        let size = contents.len() as u64;
        let key = (size, kind);
        let (reused, born) = {
            let mut g = self.inner.lock().expect("buffer pool lock");
            let born = g.frame;
            let reused = g.free.get_mut(&key).and_then(Vec::pop).map(|(b, _)| b);
            if reused.is_some() {
                g.free_bytes -= size;
            }
            (reused, born)
        };
        let buf = match reused {
            Some(buf) => {
                queue.write_buffer(&buf, 0, contents);
                buf
            }
            None => device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: kind.usage(),
            }),
        };
        PooledBuffer {
            buf: Some(buf),
            key,
            born,
            home: Arc::downgrade(&self.inner),
        }
    }
}

/// A mesh's buffer, which goes home on drop if it lived only a frame or two.
pub struct PooledBuffer {
    buf: Option<wgpu::Buffer>,
    key: Key,
    born: u64,
    home: Weak<Mutex<Inner>>,
}

impl std::ops::Deref for PooledBuffer {
    type Target = wgpu::Buffer;
    fn deref(&self) -> &wgpu::Buffer {
        self.buf.as_ref().expect("held until drop")
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        let (Some(buf), Some(home)) = (self.buf.take(), self.home.upgrade()) else {
            return;
        };
        let Ok(mut g) = home.lock() else { return };
        if g.frame.saturating_sub(self.born) <= TRANSIENT_FRAMES
            && g.free_bytes + self.key.0 <= MAX_FREE_BYTES
        {
            g.free_bytes += self.key.0;
            g.returned.push((self.key, buf));
        }
    }
}
