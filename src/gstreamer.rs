use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use bevy::prelude::*;
use bevy::{
    ecs::query::QueryItem,
    render::{
        camera::CameraUpdateSystem,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        main_graph::node::CAMERA_DRIVER,
        render_asset::{RenderAssetPlugin, RenderAssets},
        render_graph::RenderGraph,
        render_resource::Extent3d,
        renderer::RenderDevice,
        Render, RenderApp, RenderSet,
    },
};
use gstreamer::buffer::Buffer;
use gstreamer::glib;
use gstreamer::init;
use gstreamer::prelude::*;
use gstreamer::ClockTime;
use gstreamer::Element;
use gstreamer::ElementFactory;
use gstreamer::Format;
use gstreamer::MessageView;
use gstreamer::Pipeline;
use gstreamer::State;
use gstreamer_app::AppSrc;
use gstreamer_app::AppSrcCallbacks;
use gstreamer_app::AppStreamType;
use gstreamer_video::VideoFormat;
use gstreamer_video::VideoInfo;

use super::node::{ImageExportNode, NODE_NAME};
use super::plugin::get_image;
use super::plugin::ImageExportSource;

#[derive(Default)]
pub struct NDIExportPlugin;

#[derive(Component, Clone)]
pub struct NDIExport {
    inner: Arc<Mutex<NDIExportData>>,
}

pub struct NDIExportData {
    app_src: AppSrc,
    need_data: bool,
    start_time: SystemTime,
    last_frame_time: SystemTime,
    num_samples: u64,
}

impl NDIExport {
    pub fn new(instance_name: String, size: Extent3d) -> Result<Self, Box<dyn std::error::Error>> {
        init()?;
        let info = VideoInfo::builder(VideoFormat::Rgba, size.width, size.height).build()?;
        let caps = info.to_caps()?;
        let mut app_src = AppSrc::builder()
            .name("bevy_gstreamer")
            .caps(&caps)
            .block(true)
            .is_live(true)
            .do_timestamp(true)
            .stream_type(AppStreamType::Stream)
            .format(Format::Bytes)
            .build();

        //"video/x-raw, width=(int)1920, height=(int)1080, framerate=(fraction)30/1"
        // let video = ElementFactory::make("video/x-raw")
        //     .property("width", size.width)
        //     .property("height", size.height)
        //     .build()?;
        // let video = ElementFactory::make(
        //     "video/x-raw, width=(int)1920, height=(int)1080, framerate=(fraction)30/1",
        // )
        // .build()?;

        //vah264enc
        let vah264enc = ElementFactory::make("vah264enc").build()?;
        let vah264dec = ElementFactory::make("vah264dec").build()?;

        //h264parse config-interval=10
        let h264parse = ElementFactory::make("h264parse")
            .property("config-interval", 10)
            .build()?;

        //rtph264pay
        let rtph264pay = ElementFactory::make("rtph264pay").build()?;

        //udpsink host=127.0.0.1 port=5002
        let udpsink = ElementFactory::make("udpsink")
            .property("host", "127.0.0.1")
            .property("port", 5003)
            .build()?;

        let queue = ElementFactory::make("queue")
            .property_from_str("leaky", "upstream")
            .build()?;

        let xvimagesink = ElementFactory::make("xvimagesink").build()?;

        let pipeline = Pipeline::with_name("bevy_gstreamer");

        pipeline.add_many([
            app_src.upcast_ref(),
            //&video,
            &queue,
            &vah264enc,
            &vah264dec,
            &h264parse,
            &rtph264pay,
            &udpsink,
            &xvimagesink,
        ])?;

        Element::link_many([
            app_src.upcast_ref(),
            //&video,
            &queue,
            &vah264enc,
            &h264parse,
            &rtph264pay,
            &udpsink,
            //&xvimagesink,
        ])?;

        let now = SystemTime::now();
        let data = NDIExportData {
            app_src: app_src.clone(),
            start_time: now,
            last_frame_time: now,
            need_data: true,
            num_samples: 0,
        };
        let data = Arc::new(Mutex::new(data));

        let ndi_export = Self {
            inner: data.clone(),
        };

        let need_data = Arc::downgrade(&data);
        let enou_data = Arc::downgrade(&data);
        let callbacks = AppSrcCallbacks::builder()
            .need_data(move |_, _| {
                bevy::log::info!("downstream needs data!");
                let Some(need_data) = need_data.upgrade() else {
                    return;
                };
                let mut g = need_data.lock().unwrap_or_else(|e| e.into_inner());
                g.need_data = true;
                bevy::log::info!("need_data: about to dropped the mutex guard!");
                drop(g);
                bevy::log::info!("need_data: dropped the mutex guard!");
            })
            .enough_data(move |_| {
                bevy::log::info!("enough_data: downstream has enough data!");
                let Some(enou_data) = enou_data.upgrade() else {
                    return;
                };
                bevy::log::info!("enough_data: acquiring the mutex lock!");
                let mut g = enou_data.lock().unwrap_or_else(|e| e.into_inner());
                g.need_data = false;
                drop(g);
                bevy::log::info!("enough_data: dropped the mutex guard!");
            })
            .build();
        //app_src.set_callbacks(callbacks);

        let ctx = glib::MainContext::new();
        bevy::log::info!("creating glib main loop");
        let main_loop = glib::MainLoop::new(Some(&ctx), false);
        let thread_builder = std::thread::Builder::new().name("whatever".into());
        thread_builder.spawn(move || {
            let bus = pipeline.bus().unwrap();
            let main_loop_clone = main_loop.clone();
            bus.connect_message(None, move |_, msg| match msg.view() {
                MessageView::Error(err) => {
                    let main_loop = &main_loop_clone;

                    bevy::log::warn!(
                        "Error received from element {:?}: {}",
                        err.src().map(|s| s.path_string()),
                        err.error()
                    );

                    bevy::log::warn!("Debugging information: {:?}", err.debug());

                    main_loop.quit();
                }

                mv => bevy::log::info!("{mv:?}"),
            });

            bus.add_signal_watch();

            pipeline
                .set_state(State::Playing)
                .expect("Unable to set the pipeline to the `Playing` state.");

            bevy::log::info!("running main loop");
            main_loop.run();
            bevy::log::info!("main loop done");

            pipeline
                .set_state(State::Null)
                .expect("Unable to set the pipeline to the `Null` state.");

            bus.remove_signal_watch();
        })?;

        Ok(ndi_export)
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

const SAMPLE_RATE: u32 = 60; // number of samples per second

fn ndi_send_buffer(
    ndi_export_bundle: Query<(Ref<NDIExport>, Ref<Handle<ImageExportSource>>)>,
    sources: Res<RenderAssets<ImageExportSource>>,
    render_device: Res<RenderDevice>,
) {
    let sources = sources.into_inner();
    let render_device = render_device.into_inner();
    static THRESHOLD: f64 = 1.0 / SAMPLE_RATE as f64;

    for (ndi_export, source_handle) in &ndi_export_bundle {
        //bevy::log::info!("ndi_send_buffer: handling NDIExport!");
        let mut guard = ndi_export
            .as_ref()
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !guard.need_data {
            bevy::log::info!("ndi_send_buffer: doesn't need data");
            continue;
        }

        let since_frame = guard.last_frame_time.elapsed().unwrap();
        guard.last_frame_time = SystemTime::now();
        // bevy::log::info!("since last frame: {}", since_frame.as_secs_f64());
        // bevy::log::info!("       threshold: {}", THRESHOLD);
        if since_frame.as_secs_f64() < THRESHOLD {
            // bevy::log::info!("waiting until frame is scheduled to go out");
            continue;
        }

        let since_start = guard.start_time.elapsed().unwrap();
        let pts = ClockTime::NSECOND * since_start.as_nanos() as u64;
        let since_frame = ClockTime::NSECOND * since_frame.as_nanos() as u64;

        if let Some(img) = get_image(source_handle.clone(), sources, render_device) {
            let mut buffer = Buffer::from_slice(img.data);
            {
                let buffer_ref = buffer.make_mut();
                buffer_ref.set_pts(pts);
                buffer_ref.set_duration(since_frame);
            }

            // bevy::log::info!( "ndi_send_buffer: about to push buffer with size {}",
            // buffer.size());
            match guard.app_src.push_buffer(buffer) {
                Ok(_) => {
                    //bevy::log::info!("ndi_send_buffer: pushed buffer");
                }
                Err(e) => {
                    bevy::log::warn!("ndi_send_buffer: error pushing gstreamer buffer: {e:?}");
                }
            }
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

        render_app.add_systems(
            Render,
            ndi_send_buffer
                .after(RenderSet::Render)
                .before(RenderSet::Cleanup),
        );

        let mut graph = render_app.world.get_resource_mut::<RenderGraph>().unwrap();

        graph.add_node(NODE_NAME, ImageExportNode);
        graph.add_node_edge(CAMERA_DRIVER, NODE_NAME);
    }
}
