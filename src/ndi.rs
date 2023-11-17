use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::{
    ecs::query::QueryItem,
    render::{
        camera::CameraUpdateSystem,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::{RenderAssetPlugin, RenderAssets},
        renderer::RenderDevice,
        Render, RenderApp, RenderSet,
    },
};
use ndi_sdk::send::{create_ndi_send_video_frame, FrameFormatType, SendColorFormat};
use ndi_sdk::{load, SendInstance};

use super::plugin::get_image;
use super::plugin::ImageExportSource;

#[derive(Default)]
pub struct NDIExportPlugin;

#[derive(Component, Clone)]
pub struct NDIExport {
    sender: Arc<Mutex<SendInstance>>,
}

impl NDIExport {
    pub fn new(instance_name: String) -> Result<Self, Box<dyn std::error::Error>> {
        let sender = match load() {
            Err(e) => return Err(format!("failed to load NDI SDK: {e}").into()),
            Ok(instance) => match instance.create_send_instance(instance_name, false, false) {
                Err(e) => return Err(format!("failed to create NDI send instance: {e}").into()),
                Ok(sender) => sender,
            },
        };
        Ok(Self {
            sender: Arc::new(Mutex::new(sender)),
        })
    }
}

impl ExtractComponent for NDIExport {
    type Query = (&'static NDIExport, &'static Handle<ImageExportSource>);
    type Filter = ();
    type Out = (NDIExport, Handle<ImageExportSource>);

    fn extract_component((this, source_handle): QueryItem<'_, Self::Query>) -> Option<Self::Out> {
        Some((this.clone(), source_handle.clone_weak()))
    }
}

#[derive(Bundle)]
pub struct NDIExportBundle {
    pub source: Handle<ImageExportSource>,
    pub export: NDIExport,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum NDIExportSystems {
    SetupNDIExport,
    SetupNDIExportFlush,
}

fn ndi_send_buffer(
    ndi_export_bundle: Query<(Ref<NDIExport>, Ref<Handle<ImageExportSource>>)>,
    sources: Res<RenderAssets<ImageExportSource>>,
    render_device: Res<RenderDevice>,
    time: Res<Time>,
    mut timer: ResMut<NDIExportRateLimiter>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    let sources = sources.into_inner();
    let render_device = render_device.into_inner();

    for (ndi_export, source_handle) in &ndi_export_bundle {
        if let Some(img) = get_image(source_handle.clone(), sources, render_device) {
            if let Ok(dy) = img.clone().try_into_dynamic() {
                dbg!("saving");
                dy.save("test.png").ok();
            }
            println!("building NDISendVideoFrame");
            println!("img buf len: {}", img.data.len());
            println!("img buf width: {}", img.width());
            println!("img buf height: {}", img.height());
            let (x, y) = (img.width() as i32, img.height() as i32);
            let frame_builder = create_ndi_send_video_frame(x, y, FrameFormatType::Progressive)
                .with_data(img.data, x * 4, SendColorFormat::Rgba);
            let frame = match frame_builder.build() {
                Err(e) => {
                    eprintln!("failed to build NDISendVideoFrame: {e}");
                    return;
                }
                Ok(f) => f,
            };
            println!("exporting video frame to NDI namespace");
            ndi_export
                .sender
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .send_video(frame);
        }
    }
}

#[derive(Resource)]
struct NDIExportRateLimiter(Timer);

impl Plugin for NDIExportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            (
                NDIExportSystems::SetupNDIExport,
                NDIExportSystems::SetupNDIExportFlush,
            )
                .chain()
                .before(CameraUpdateSystem),
        )
        .register_type::<ImageExportSource>()
        .init_asset::<ImageExportSource>()
        .register_asset_reflect::<ImageExportSource>()
        .add_plugins((
            RenderAssetPlugin::<ImageExportSource>::default(),
            ExtractComponentPlugin::<NDIExport>::default(),
        ));

        let render_app = app.sub_app_mut(RenderApp);

        render_app
            .insert_resource(NDIExportRateLimiter(Timer::from_seconds(
                1.0,
                TimerMode::Repeating,
            )))
            .add_systems(
                Render,
                ndi_send_buffer
                    .after(RenderSet::Render)
                    .before(RenderSet::Cleanup),
            );
    }
}
