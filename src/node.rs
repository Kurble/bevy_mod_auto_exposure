use std::sync::{Arc, Mutex};

use bevy::{
    ecs::{query::QueryState, system::lifetimeless::Read, world::World},
    render::{
        render_asset::RenderAssets,
        render_graph::*,
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        texture::{FallbackImage, Image},
        view::{ExtractedView, ViewTarget},
    },
};

use crate::pipeline::{AutoExposureParams, AutoExposurePipeline, ViewAutoExposurePipeline};

pub struct MeteringNode {
    query: QueryState<(
        Read<ViewTarget>,
        Read<ViewAutoExposurePipeline>,
        Read<ExtractedView>,
    )>,
    cached_bind_groups: Mutex<Option<CachedBindGroup>>,
    histogram: Buffer,


    average: Texture,
    average_view: TextureView,
    last_average: Arc<Mutex<Average>>,
}

pub struct Average {
    buffer: Buffer,
    value: f32,
}

struct CachedBindGroup {
    source_id: TextureViewId,
    mask_id: TextureViewId,
    compute_bind_group: BindGroup,
}

impl MeteringNode {
    pub const NAME: &'static str = "auto_exposure";

    pub const IN_VIEW: &'static str = "view";

    pub fn new(world: &mut World) -> Self {
        let last_average = Arc::new(Mutex::new(Average {
            buffer: world
                .resource::<RenderDevice>()
                .create_buffer(&BufferDescriptor {
                    label: Some("last average luminance"),
                    size: 4,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            value: 0.0,
        }));

        let average = world
            .resource::<RenderDevice>()
            .create_texture(&TextureDescriptor {
                label: Some("average luminance"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R16Float,
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });

        let average_view = average.create_view(&Default::default());

        Self {
            query: QueryState::new(world),
            cached_bind_groups: Mutex::new(None),
            histogram: world
                .resource::<RenderDevice>()
                .create_buffer(&BufferDescriptor {
                    label: Some("histogram buffer"),
                    size: 256 * 4,
                    usage: BufferUsages::STORAGE,
                    mapped_at_creation: false,
                }),
            average,
            average_view,
            last_average,
        }
    }
}

impl Node for MeteringNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![SlotInfo::new(MeteringNode::IN_VIEW, SlotType::Entity)]
    }

    fn update(&mut self, world: &mut World) {
        self.query.update_archetypes(world);
    }

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let view_entity = graph.get_input_entity(Self::IN_VIEW)?;
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<AutoExposurePipeline>();

        let (view_target, auto_exposure, view) = match self.query.get_manual(world, view_entity) {
            Ok(result) => result,
            Err(_) => return Ok(()),
        };

        if !view_target.is_hdr() {
            return Ok(());
        }

        let Some(histogram_pipeline) =
            pipeline_cache.get_compute_pipeline(auto_exposure.histogram_pipeline)
        else {
            return Ok(());
        };

        let Some(average_pipeline) =
            pipeline_cache.get_compute_pipeline(auto_exposure.mean_luminance_pipeline)
        else {
            return Ok(());
        };

        let source = view_target.main_texture_view();

        let fallback = world.resource::<FallbackImage>();
        let mask = world
            .resource::<RenderAssets<Image>>()
            .get(&auto_exposure.metering_mask);
        let mask = mask
            .map(|i| &i.texture_view)
            .unwrap_or(&fallback.d2.texture_view);

        let mut cached_texture_bind_group = self.cached_bind_groups.lock().unwrap();
        let compute_bind_group = match &mut *cached_texture_bind_group {
            Some(cache) if source.id() == cache.source_id && mask.id() == cache.mask_id => {
                &cache.compute_bind_group
            }
            cached_bind_group => {
                let mut settings = encase::UniformBuffer::new(Vec::new());
                settings
                    .write(&AutoExposureParams {
                        min_log_lum: auto_exposure.min,
                        inv_log_lum_range: 1.0 / (auto_exposure.max - auto_exposure.min),
                        log_lum_range: auto_exposure.max - auto_exposure.min,
                        num_pixels: (view.viewport.z * view.viewport.w) as f32,
                        delta_time: 0.01,
                        correction: auto_exposure.correction,
                    })
                    .unwrap();
                let settings =
                    render_context
                        .render_device()
                        .create_buffer_with_data(&BufferInitDescriptor {
                            label: None,
                            contents: settings.as_ref(),
                            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                        });

                let compute_bind_group = render_context.render_device().create_bind_group(
                    None,
                    &pipeline.histogram_layout,
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: settings.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(source),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: BindingResource::TextureView(mask),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: self.histogram.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 4,
                            resource: BindingResource::TextureView(&self.average_view),
                        },
                    ],
                );

                let cache = cached_bind_group.insert(CachedBindGroup {
                    source_id: source.id(),
                    mask_id: mask.id(),
                    compute_bind_group,
                });

                &cache.compute_bind_group
            }
        };

        let mut compute_pass =
            render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("metering_pass"),
                });

        compute_pass.set_bind_group(0, &compute_bind_group, &[]);
        compute_pass.set_pipeline(histogram_pipeline);
        compute_pass.dispatch_workgroups(
            (view.viewport.z + 15) / 16,
            (view.viewport.w + 15) / 16,
            1,
        );
        compute_pass.set_pipeline(average_pipeline);
        compute_pass.dispatch_workgroups(1, 1, 1);

        drop(compute_pass);

        let average_clone = self.last_average.clone();
        if let Ok(average) = self.last_average.lock() {
            render_context.command_encoder().copy_texture_to_buffer(
                ImageCopyTexture {
                    texture: &self.average,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                ImageCopyBuffer {
                    buffer: &average.buffer,
                    layout: ImageDataLayout {
                        offset: 0,
                        bytes_per_row: None,
                        rows_per_image: None,
                    },
                },
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );

            render_context.render_device().map_buffer(
                &average.buffer.slice(..),
                MapMode::Read,
                move |result| {
                    if let (Ok(()), Ok(mut average)) = (result, average_clone.lock()) {
                        average.update();
                    }
                },
            );
        }

        Ok(())
    }
}

impl Average {
    fn update(&mut self) {
        let data = <[u8; 2]>::try_from(self.buffer.slice(..).get_mapped_range().as_ref()).unwrap();
        self.buffer.unmap();

        self.value = half::f16::from_bits(u16::from_le_bytes(data)).to_f32();

        // convert from f16 to f32
        //self.value = f16::from_bits(u16::from_le_bytes(data)).to_f32();
    }
}
