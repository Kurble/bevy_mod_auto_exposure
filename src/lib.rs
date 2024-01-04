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
use pipeline::{AutoExposurePipeline, Pass, ViewAutoExposurePipeline};

use crate::node::MeteringNode;

mod node;
mod pipeline;

pub struct AutoExposurePlugin;

#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component)]
pub struct AutoExposure {
    pub min: f32,
    pub max: f32,
    pub correction: f32,
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
        embedded_asset!(app, "src/", "metering.wgsl");

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
            .add_render_graph_node::<MeteringNode>(CORE_3D, node::MeteringNode::NAME)
            .add_render_graph_edges(
                CORE_3D,
                &[END_MAIN_PASS, node::MeteringNode::NAME, TONEMAPPING],
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
                size: 16,
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
                        size: 16,
                        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    })
                })
                .clone(),
            min: auto_exposure.min,
            max: auto_exposure.max,
            correction: auto_exposure.correction,
            metering_mask: auto_exposure.metering_mask.clone(),
        });
    }
}
