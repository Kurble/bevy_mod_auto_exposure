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
        render_graph::{RenderGraph, RenderGraphApp},
        render_resource::SpecializedComputePipelines,
        Render, RenderApp, RenderSet,
    },
};

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
            .init_resource::<SpecializedComputePipelines<pipeline::AutoExposurePipeline>>()
            .add_systems(
                Render,
                pipeline::queue_view_auto_exposure_pipelines.in_set(RenderSet::Queue),
            );

        render_app
            .add_render_graph_node::<MeteringNode>(CORE_3D, node::MeteringNode::NAME)
            .add_render_graph_edges(
                CORE_3D,
                &[END_MAIN_PASS, node::MeteringNode::NAME, TONEMAPPING],
            );

        // let mut render_graph = render_app.world.resource_mut::<RenderGraph>();
        // let Some(draw_3d_graph) =
        //     render_graph.get_sub_graph_mut(bevy::core_pipeline::core_3d::graph::NAME)
        // else {
        //     return;
        // };

        // let input_node_id = draw_3d_graph.input_node().id;

        //                 //.add_render_graph_node::<AutoExposureNode>(CORE_3D, AUTOEXPOSURE)
        // draw_3d_graph.add_node(node::MeteringNode::NAME, metering_node);

        // draw_3d_graph.add_slot_edge(
        //     input_node_id,
        //     bevy::core_pipeline::core_3d::graph::input::VIEW_ENTITY,
        //     node::MeteringNode::NAME,
        //     node::MeteringNode::IN_VIEW,
        // );
        // draw_3d_graph.add_node_edge(
        //     bevy::core_pipeline::core_3d::graph::node::END_MAIN_PASS,
        //     node::MeteringNode::NAME,
        // );
    }

    fn finish(&self, app: &mut App) {
        let Ok(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<pipeline::AutoExposurePipeline>();
    }
}
