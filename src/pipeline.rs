use std::num::NonZeroU64;

use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};

#[derive(Resource)]
pub struct AutoExposurePipeline {
    pub histogram_layout: BindGroupLayout,
    pub histogram_shader: Option<Handle<Shader>>,
}

#[derive(Component)]
pub struct ViewAutoExposurePipeline {
    pub histogram_pipeline: CachedComputePipelineId,
    pub mean_luminance_pipeline: CachedComputePipelineId,
    pub state: Buffer,
    pub min: f32,
    pub max: f32,
    pub correction: f32,
    pub metering_mask: Handle<Image>,
}

#[derive(ShaderType)]
pub struct AutoExposureParams {
    pub min_log_lum: f32,
    pub inv_log_lum_range: f32,
    pub log_lum_range: f32,
    pub num_pixels: f32,
    pub delta_time: f32,
    pub correction: f32,
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum Pass {
    Histogram,
    Average,
}

impl FromWorld for AutoExposurePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let asset_server = world.resource::<AssetServer>();

        Self {
            histogram_layout: render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("compute histogram bind group"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(AutoExposureParams::min_size()),
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Float { filterable: false },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Float { filterable: false },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 3,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(256 * 4),
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 4,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(16),
                        },
                        count: None,
                    },
                ],
            }),
            histogram_shader: Some(
                asset_server.load("embedded://bevy_mod_auto_exposure/metering.wgsl"),
            ),
        }
    }
}

impl SpecializedComputePipeline for AutoExposurePipeline {
    type Key = Pass;

    fn specialize(&self, pass: Pass) -> ComputePipelineDescriptor {
        ComputePipelineDescriptor {
            label: Some("luminance compute pipeline".into()),
            layout: vec![self.histogram_layout.clone()],
            shader: self.histogram_shader.clone().unwrap(),
            shader_defs: vec![],
            entry_point: match pass {
                Pass::Histogram => "computeHistogram".into(),
                Pass::Average => "computeAverage".into(),
            },
            push_constant_ranges: vec![],
        }
    }
}
