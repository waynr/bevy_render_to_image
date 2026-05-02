use bevy::{
    ecs::{
        query::QueryItem,
        system::{lifetimeless::SRes, SystemParamItem},
    },
    prelude::*,
    reflect::TypeUuid,
    render::{
        extract_component::ExtractComponent,
        render_asset::{PrepareAssetError, RenderAsset, RenderAssets},
        render_resource::{Buffer, BufferDescriptor, BufferUsages, Extent3d, MapMode},
        renderer::RenderDevice,
    },
};
use futures::channel::oneshot;
use wgpu::Maintain;

#[derive(Clone, TypeUuid, Default, Reflect, Asset)]
#[uuid = "d619b2f8-58cf-42f6-b7da-028c0595f7aa"]
pub struct ImageExportSource(pub Handle<Image>);

impl From<Handle<Image>> for ImageExportSource {
    fn from(value: Handle<Image>) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct GpuImageExportSource {
    pub buffer: Buffer,
    pub source_handle: Handle<Image>,
    pub source_size: Extent3d,
    pub bytes_per_row: u32,
    pub padded_bytes_per_row: u32,
}

impl RenderAsset for ImageExportSource {
    type ExtractedAsset = Self;
    type PreparedAsset = GpuImageExportSource;
    type Param = (SRes<RenderDevice>, SRes<RenderAssets<Image>>);

    fn extract_asset(&self) -> Self::ExtractedAsset {
        self.clone()
    }

    fn prepare_asset(
        extracted_asset: Self::ExtractedAsset,
        (device, images): &mut SystemParamItem<Self::Param>,
    ) -> Result<Self::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>> {
        let gpu_image = images.get(&extracted_asset.0).unwrap();

        let size = gpu_image.texture.size();
        let format = &gpu_image.texture_format;
        let bytes_per_row =
            (size.width / format.block_dimensions().0) * format.block_size(None).unwrap();
        let padded_bytes_per_row =
            RenderDevice::align_copy_bytes_per_row(bytes_per_row as usize) as u32;

        let source_size = gpu_image.texture.size();

        Ok(GpuImageExportSource {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some("Image Export Buffer"),
                size: (source_size.height * padded_bytes_per_row) as u64,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            source_handle: extracted_asset.0.clone(),
            source_size,
            bytes_per_row,
            padded_bytes_per_row,
        })
    }
}

#[derive(Component, Clone, Copy, Default)]
pub struct ImageExport;

impl ExtractComponent for ImageExport {
    type Query = (&'static ImageExport, &'static Handle<ImageExportSource>);
    type Filter = ();
    type Out = (ImageExport, Handle<ImageExportSource>);

    fn extract_component((this, source_handle): QueryItem<'_, Self::Query>) -> Option<Self::Out> {
        //dbg!("extract");
        Some((*this, source_handle.clone_weak()))
    }
}

#[derive(Bundle, Default)]
pub struct ImageExportBundle {
    pub source: Handle<ImageExportSource>,
    pub export: ImageExport,
}

pub(crate) fn get_image(
    source_handle: Handle<ImageExportSource>,
    sources: &RenderAssets<ImageExportSource>,
    render_device: &RenderDevice,
) -> Option<Image> {
    //dbg!(&source_handle);
    if let Some(gpu_source) = sources.get(source_handle.id()) {
        let mut image_bytes = {
            let slice = gpu_source.buffer.slice(..);

            {
                let (mapping_tx, mapping_rx) = oneshot::channel();

                render_device.map_buffer(&slice, MapMode::Read, move |res| {
                    mapping_tx.send(res).unwrap();
                });

                render_device.poll(Maintain::Wait);
                futures_lite::future::block_on(mapping_rx).unwrap().unwrap();
            }

            slice.get_mapped_range().to_vec()
        };

        gpu_source.buffer.unmap();

        let bytes_per_row = gpu_source.bytes_per_row as usize;
        let padded_bytes_per_row = gpu_source.padded_bytes_per_row as usize;
        let source_size = gpu_source.source_size;

        if bytes_per_row != padded_bytes_per_row {
            let mut unpadded_bytes =
                Vec::<u8>::with_capacity(source_size.height as usize * bytes_per_row);

            for padded_row in image_bytes.chunks(padded_bytes_per_row) {
                unpadded_bytes.extend_from_slice(&padded_row[..bytes_per_row]);
            }

            image_bytes = unpadded_bytes;
        }
        //dbg!(image_bytes.len());

        let img = Image {
            data: image_bytes,
            texture_descriptor: wgpu::TextureDescriptor {
                size: wgpu::Extent3d {
                    width: source_size.width,
                    height: source_size.height,
                    depth_or_array_layers: 1,
                },
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                dimension: wgpu::TextureDimension::D2,
                label: None,
                mip_level_count: 1,
                sample_count: 1,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            sampler: bevy::render::texture::ImageSampler::Default,
            texture_view_descriptor: None,
        };
        return Some(img);
    }
    None
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum ImageExportSystems {
    SetupImageExport,
    SetupImageExportFlush,
}
