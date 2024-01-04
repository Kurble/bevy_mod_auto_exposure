use bevy::{
    asset::embedded_asset,
    core_pipeline::core_3d::{
        graph::node::{END_MAIN_PASS, TONEMAPPING},
        CORE_3D,
    },
    ecs::{query::QueryItem, system::lifetimeless::Read},
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_graph::RenderGraphApp,
        render_resource::{
            Buffer, BufferDescriptor, BufferUsages, PipelineCache, SpecializedComputePipelines,
        },
        renderer::RenderDevice,
        Extract, Render, RenderApp, RenderSet,
    },
    utils::HashMap,
};
use pipeline::{AutoExposurePipeline, Pass, ViewAutoExposurePipeline, AutoExposureParams};

use crate::node::AutoExposureNode;

mod node;
mod pipeline;

pub struct AutoExposurePlugin;

#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
pub struct AutoExposure {
    /// The minimum exposure value for the camera.
    pub min: f32,
    /// The maximum exposure value for the camera.
    pub max: f32,
    /// Global exposure correction, after metering.
    pub correction: f32,
    /// The percentage of darkest pixels to ignore when metering.
    pub low_percent: u32,
    /// The percentage of brightest pixels to ignore when metering.
    pub high_percent: u32,
    /// The speed at which the exposure adapts from dark to bright scenes.
    pub speed_up: f32,
    /// The speed at which the exposure adapts from bright to dark scenes.
    pub speed_down: f32,
    /// The mask to apply when metering. Bright spots on the mask will contribute more to the
    /// metering, and dark spots will contribute less.
    pub metering_mask: Handle<Image>,
}

#[derive(Resource)]
pub struct AutoExposureResources {
    pub histogram: Buffer,
}

#[derive(Resource)]
pub struct ExtractedAutoExposureBuffers {
    pub changed: Vec<(Entity,)>,
    pub removed: Vec<Entity>,
}

#[derive(Resource, Default)]
pub struct AutoExposureBuffers {
    pub buffers: HashMap<Entity, Buffer>,
}

impl Default for AutoExposure {
    fn default() -> Self {
        Self {
            min: -8.0,
            max: 8.0,
            correction: 0.0,
            low_percent: 60,
            high_percent: 95,
            speed_up: 3.0,
            speed_down: 1.0,
            metering_mask: default(),
        }
    }
}

impl ExtractComponent for AutoExposure {
    type Query = Read<Self>;
    type Filter = With<Camera>;
    type Out = Self;

    fn extract_component(item: QueryItem<'_, Self::Query>) -> Option<Self> {
        Some(item.clone())
    }
}

impl Plugin for AutoExposurePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "src/", "auto_exposure.wgsl");

        app.register_type::<AutoExposure>();
        app.add_plugins(ExtractComponentPlugin::<AutoExposure>::default());

        let Ok(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<SpecializedComputePipelines<AutoExposurePipeline>>()
            .init_resource::<AutoExposureBuffers>()
            .add_systems(ExtractSchedule, extract_auto_exposure_buffers)
            .add_systems(
                Render,
                (
                    prepare_auto_exposure_buffers.in_set(RenderSet::Prepare),
                    queue_view_auto_exposure_pipelines.in_set(RenderSet::Queue),
                ),
            )
            .add_render_graph_node::<AutoExposureNode>(CORE_3D, node::AutoExposureNode::NAME)
            .add_render_graph_edges(
                CORE_3D,
                &[END_MAIN_PASS, node::AutoExposureNode::NAME, TONEMAPPING],
            );
    }

    fn finish(&self, app: &mut App) {
        let Ok(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<AutoExposurePipeline>();
        render_app.init_resource::<AutoExposureResources>();
    }
}

impl FromWorld for AutoExposureResources {
    fn from_world(world: &mut World) -> Self {
        Self {
            histogram: world
                .resource::<RenderDevice>()
                .create_buffer(&BufferDescriptor {
                    label: Some("histogram buffer"),
                    size: 256 * 4,
                    usage: BufferUsages::STORAGE,
                    mapped_at_creation: false,
                }),
        }
    }
}

pub fn extract_auto_exposure_buffers(
    mut commands: Commands,
    changed: Extract<Query<Entity, Added<AutoExposure>>>,
    mut removed: Extract<RemovedComponents<AutoExposure>>,
) {
    commands.insert_resource(ExtractedAutoExposureBuffers {
        changed: changed.iter().map(|entity| (entity,)).collect(),
        removed: removed.read().collect(),
    });
}

pub fn prepare_auto_exposure_buffers(
    device: Res<RenderDevice>,
    mut extracted: ResMut<ExtractedAutoExposureBuffers>,
    mut buffers: ResMut<AutoExposureBuffers>,
) {
    for entity in extracted.changed.drain(..).map(|(entity,)| entity) {
        buffers.buffers.insert(
            entity,
            device.create_buffer(&BufferDescriptor {
                label: Some("auto exposure state buffer"),
                size: 4,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
        );
    }

    for entity in extracted.removed.drain(..) {
        buffers.buffers.remove(&entity);
    }
}

pub fn queue_view_auto_exposure_pipelines(
    mut commands: Commands,
    mut pipeline_cache: ResMut<PipelineCache>,
    mut compute_pipelines: ResMut<SpecializedComputePipelines<AutoExposurePipeline>>,
    device: Res<RenderDevice>,
    pipeline: Res<AutoExposurePipeline>,
    time: Res<Time>,
    mut buffers: ResMut<AutoExposureBuffers>,
    view_targets: Query<(Entity, &AutoExposure)>,
) {
    for (entity, auto_exposure) in view_targets.iter() {
        let histogram_pipeline =
            compute_pipelines.specialize(&mut pipeline_cache, &pipeline, Pass::Histogram);
        let average_pipeline =
            compute_pipelines.specialize(&mut pipeline_cache, &pipeline, Pass::Average);

        commands.entity(entity).insert(ViewAutoExposurePipeline {
            histogram_pipeline,
            mean_luminance_pipeline: average_pipeline,
            state: buffers
                .buffers
                .entry(entity)
                .or_insert_with(|| {
                    device.create_buffer(&BufferDescriptor {
                        label: Some("auto exposure state buffer"),
                        size: 4,
                        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    })
                })
                .clone(),
            params: AutoExposureParams {
                min_log_lum: auto_exposure.min,
                inv_log_lum_range: 1.0 / (auto_exposure.max - auto_exposure.min),
                log_lum_range: auto_exposure.max - auto_exposure.min,
                correction: auto_exposure.correction,
                low_percent: auto_exposure.low_percent,
                high_percent: auto_exposure.high_percent,
                speed_up: auto_exposure.speed_up * time.delta_seconds(),
                speed_down: auto_exposure.speed_down * time.delta_seconds(),
            },
            metering_mask: auto_exposure.metering_mask.clone(),
        });
    }
}
