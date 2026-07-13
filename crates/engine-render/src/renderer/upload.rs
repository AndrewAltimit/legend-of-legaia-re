//! `Renderer` GPU-upload methods: mesh / textured-mesh / VRAM-mesh /
//! color-mesh / lines / font-atlas / texture uploads. Split out of
//! `renderer.rs`.

use super::*;

impl Renderer {
    pub fn upload_mesh(&self, positions: &[[f32; 3]], indices: &[u32]) -> Result<UploadedMesh> {
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!("mesh index count {} is not a multiple of 3", indices.len());
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!("mesh index {} >= vertex count {}", max_idx, positions.len());
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertex buffer"),
                contents: bytemuck::cast_slice(positions),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a textured mesh: positions + UVs (paired by index, same length)
    /// plus triangle indices. Vertex+UV data is interleaved as `[x,y,z,u,v]`
    /// so it matches the textured-mesh pipeline's vertex layout in one buffer.
    pub fn upload_textured_mesh(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[f32; 2]],
        indices: &[u32],
    ) -> Result<UploadedTexturedMesh> {
        if positions.len() != uvs.len() {
            anyhow::bail!(
                "textured mesh: positions ({}) and uvs ({}) length mismatch",
                positions.len(),
                uvs.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "textured mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "textured mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        // Interleave: [x,y,z,u,v] per vertex (5 f32 = 20 bytes, matches the
        // pipeline's 20-byte stride).
        let mut interleaved = Vec::with_capacity(positions.len() * 5);
        for (p, uv) in positions.iter().zip(uvs.iter()) {
            interleaved.push(p[0]);
            interleaved.push(p[1]);
            interleaved.push(p[2]);
            interleaved.push(uv[0]);
            interleaved.push(uv[1]);
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("textured mesh vertex buffer"),
                contents: bytemuck::cast_slice(&interleaved),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("textured mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedTexturedMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a CPU-side [`Vram`] as a 1024×512 R16Uint texture. The fragment
    /// shader reads from it via `textureLoad` (no sampler - Uint textures
    /// aren't filterable on most backends, and PSX texture lookup is
    /// integer-exact anyway).
    pub fn upload_vram(&self, vram: &Vram) -> Result<UploadedVram> {
        let size = wgpu::Extent3d {
            width: VRAM_WIDTH as u32,
            height: VRAM_HEIGHT as u32,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("psx vram"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            vram.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(VRAM_WIDTH as u32 * 2),
                rows_per_image: Some(VRAM_HEIGHT as u32),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("psx vram bg"),
            layout: &self.vram_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let generation = self.vram_upload_counter.get() + 1;
        self.vram_upload_counter.set(generation);
        Ok(UploadedVram {
            bind_group,
            generation,
        })
    }

    /// Upload a VRAM mesh: position + per-vertex `(u, v)` (each 0..255) +
    /// per-vertex `(cba, tsb)` PSX VRAM addresses, plus triangle indices.
    /// Vertex layout matches the VRAM-mesh pipeline's 20-byte stride.
    pub fn upload_vram_mesh(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[u8; 2]],
        cba_tsb: &[[u16; 2]],
        normals: &[[f32; 3]],
        indices: &[u32],
    ) -> Result<UploadedVramMesh> {
        if positions.len() != uvs.len()
            || positions.len() != cba_tsb.len()
            || positions.len() != normals.len()
        {
            anyhow::bail!(
                "vram mesh attribute length mismatch: pos={} uvs={} cba_tsb={} normals={}",
                positions.len(),
                uvs.len(),
                cba_tsb.len(),
                normals.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "vram mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "vram mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        let mut bytes = Vec::with_capacity(positions.len() * 32);
        for (((pos, uv), ct), n) in positions
            .iter()
            .zip(uvs.iter())
            .zip(cba_tsb.iter())
            .zip(normals.iter())
        {
            bytes.extend_from_slice(bytemuck::cast_slice(pos));
            // UV padded to 4 bytes (Uint8x4 - extra bytes ignored by shader).
            bytes.push(uv[0]);
            bytes.push(uv[1]);
            bytes.push(0);
            bytes.push(0);
            bytes.extend_from_slice(&ct[0].to_le_bytes());
            bytes.extend_from_slice(&ct[1].to_le_bytes());
            bytes.extend_from_slice(bytemuck::cast_slice(n));
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vram mesh vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Append the per-ABR-mode semi-transparent tail for the PSX-faithful
        // blend pass. The opaque pass still draws `0..indices.len()`
        // (`index_count` below), so the default path is unchanged.
        let (indices_with_tail, semi_ranges, semi_prims) =
            psx_blend::append_semi_tail(indices, cba_tsb, positions);
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vram mesh index buffer"),
                contents: bytemuck::cast_slice(&indices_with_tail),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedVramMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
            semi_ranges,
            semi_prims,
        })
    }

    /// Upload an untextured vertex-colour triangle mesh: position + per-vertex
    /// `[r, g, b]` (each 0..255, alpha forced opaque) + triangle indices. The
    /// inverse of [`Self::upload_vram_mesh`] for the `F*`/`G*` props that carry
    /// colours instead of UVs ([`legaia_tmd::mesh::ColorMesh`]). Every prim is
    /// treated as opaque; use [`Self::upload_color_mesh_blended`] when the
    /// source prims carry semi-transparency (ABE) state.
    pub fn upload_color_mesh(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 3]],
        indices: &[u32],
    ) -> Result<UploadedColorMesh> {
        self.upload_color_mesh_blended(positions, colors, indices, &[])
    }

    /// [`Self::upload_color_mesh`] plus per-vertex PSX semi-transparency
    /// state. `blend` is index-aligned with `positions`: each entry is a
    /// blend word packing the prim's ABE enable into bit 15 and its ABR
    /// blend mode into bits 5..=6 ([`psx_blend::pack_blend_word`] - the
    /// same packing the textured path rides on the TSB attribute). All
    /// corners of a triangle must share one word (the mesh builders emit
    /// fresh per-corner vertices per prim). An empty slice means "all
    /// opaque" and is what [`Self::upload_color_mesh`] passes.
    ///
    /// Semi-transparent triangles are duplicated into a per-ABR-mode index
    /// tail ([`psx_blend::append_semi_tail_words`]) drawn by the
    /// PSX-faithful blend pass; on real hardware an untextured ABE prim
    /// blends **all** its pixels (there is no per-texel STP gate), so in
    /// PSX mode the opaque pass discards those prims entirely and the
    /// blend pass owns them. The default (non-PSX) path still draws
    /// everything opaque, unchanged.
    pub fn upload_color_mesh_blended(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 3]],
        indices: &[u32],
        blend: &[u16],
    ) -> Result<UploadedColorMesh> {
        if positions.len() != colors.len() {
            anyhow::bail!(
                "color mesh: positions ({}) and colors ({}) length mismatch",
                positions.len(),
                colors.len()
            );
        }
        if !blend.is_empty() && blend.len() != positions.len() {
            anyhow::bail!(
                "color mesh: positions ({}) and blend words ({}) length mismatch",
                positions.len(),
                blend.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "color mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "color mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        let mut bytes = Vec::with_capacity(positions.len() * 20);
        for (i, (pos, c)) in positions.iter().zip(colors.iter()).enumerate() {
            bytes.extend_from_slice(bytemuck::cast_slice(pos));
            bytes.push(c[0]);
            bytes.push(c[1]);
            bytes.push(c[2]);
            bytes.push(0xFF); // opaque alpha (Unorm8x4)
            let word = blend.get(i).copied().unwrap_or(0) as u32;
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("color mesh vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Append the per-ABR-mode semi-transparent tail for the PSX-faithful
        // blend pass. The opaque pass still draws `0..indices.len()`
        // (`index_count` below), so the default path is unchanged.
        let (indices_with_tail, semi_ranges, semi_prims) = if blend.is_empty() {
            (indices.to_vec(), [(0u32, 0u32); 4], Vec::new())
        } else {
            psx_blend::append_semi_tail_words(indices, blend, positions)
        };
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("color mesh index buffer"),
                contents: bytemuck::cast_slice(&indices_with_tail),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedColorMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
            semi_ranges,
            semi_prims,
        })
    }

    /// Upload a wireframe line mesh: position + per-vertex `[r, g, b, a]`
    /// (each 0..255), plus line indices. Indices form a `LineList`: every
    /// 2 indices = 1 segment.
    pub fn upload_lines(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 4]],
        indices: &[u32],
    ) -> Result<UploadedLines> {
        if positions.len() != colors.len() {
            anyhow::bail!(
                "lines: positions ({}) and colors ({}) length mismatch",
                positions.len(),
                colors.len()
            );
        }
        if !indices.len().is_multiple_of(2) {
            anyhow::bail!(
                "lines: index count {} is not a multiple of 2",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "lines: index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        // Interleave pos (12) + color (4) = 16 bytes/vertex.
        let mut bytes = Vec::with_capacity(positions.len() * 16);
        for (p, c) in positions.iter().zip(colors.iter()) {
            bytes.extend_from_slice(bytemuck::cast_slice(p));
            bytes.extend_from_slice(c);
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedLines {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a [`legaia_font::Font`]'s atlas to the GPU. Convenience wrapper
    /// around [`Self::upload_font_atlas`] that pulls dimensions and pixels
    /// from the parsed font directly. Use this when the caller is loading
    /// the dialog font; use the lower-level `upload_font_atlas` for custom
    /// atlases (debug fonts, sprite glyph sheets, etc).
    pub fn upload_font(&self, font: &legaia_font::Font) -> Result<UploadedFontAtlas> {
        let (w, h) = font.atlas_dimensions();
        self.upload_font_atlas(font.atlas_rgba(), w, h)
    }

    /// Upload a sprite atlas. Alias of [`Self::upload_font_atlas`] - sprites
    /// and font glyphs share the textured-quad pipeline (see [`SpriteDraw`]).
    pub fn upload_sprite_atlas(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<UploadedSpriteAtlas> {
        self.upload_font_atlas(rgba, width, height)
    }

    /// Upload a font atlas. Used by the 2D text pipeline; one atlas can back
    /// many [`TextOverlay`] batches.
    pub fn upload_font_atlas(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<UploadedFontAtlas> {
        if rgba.len() as u32 != width * height * 4 {
            anyhow::bail!(
                "font atlas RGBA length {} doesn't match {}x{} (expected {})",
                rgba.len(),
                width,
                height,
                width * height * 4
            );
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // UNORM: the atlas holds PSX glyph bytes and the attachment is
            // UNORM too, so the texel must reach the blend unconverted. An
            // sRGB source would be decoded to linear on sample and then
            // written verbatim, darkening every glyph.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("font atlas bg"),
            layout: &self.text_atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.text_sampler),
                },
            ],
        });
        Ok(UploadedFontAtlas {
            bind_group,
            width,
            height,
        })
    }

    pub fn upload_texture(&self, rgba: &[u8], width: u32, height: u32) -> Result<UploadedTexture> {
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            anyhow::bail!(
                "rgba length mismatch: got {}, expected {}",
                rgba.len(),
                expected
            );
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uploaded texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // UNORM: these are TIM-decoded PSX texels (display-referred), and
            // the attachment is UNORM - no colour-space conversion anywhere on
            // the path (see `choose_surface_format`).
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Ok(UploadedTexture {
            bind_group,
            width,
            height,
        })
    }
}
