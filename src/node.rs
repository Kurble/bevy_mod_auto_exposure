use std::sync::Mutex;

use bevy::{
    ecs::{query::QueryState, system::lifetimeless::Read, world::{World, FromWorld}},
    render::{
        render_asset::RenderAssets,
        render_graph::*,
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        texture::{FallbackImage, Image},
        view::{ExtractedView, ViewTarget, ViewUniforms},
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
    average: Buffer,
}

struct CachedBindGroup {
    source_id: TextureViewId,
    mask_id: TextureViewId,
    compute_bind_group: BindGroup,
}

impl MeteringNode {
    pub const NAME: &'static str = "auto_exposure";

    pub const IN_VIEW: &'static str = "view";
}

impl FromWorld for MeteringNode {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let histogram = render_device.create_buffer(&BufferDescriptor {
            label: Some("histogram buffer"),
            size: 256 * 4,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let average = render_device.create_buffer(&BufferDescriptor {
            label: Some("average luminance"),
            size: 4,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        Self {
            query: QueryState::new(world),
            cached_bind_groups: Mutex::new(None),
            histogram,
            average,
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
                            resource: self.average.as_entire_binding(),
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

        // Copy the computed exposure value to the view uniforms.
        // If this wasn't a plugin, we could just add the STORAGE access modifier to the view uniforms buffer
        // and write directly to it. But since this is a plugin, we have to resort to this hack.
        if let Some(view_uniforms_buffer) = world.resource::<ViewUniforms>().uniforms.buffer() {
            let exposure_offset = 0x230;
            render_context.command_encoder().copy_buffer_to_buffer(
                &self.average,
                0,
                &view_uniforms_buffer,
                exposure_offset,
                4,
            );
        }

        Ok(())
    }
}
