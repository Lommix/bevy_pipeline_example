use bevy::{
    core_pipeline::core_2d::{Transparent2d, CORE_2D_DEPTH_FORMAT},
    ecs::{
        query::ROQueryItem,
        system::{
            lifetimeless::{Read, SRes},
            SystemParamItem,
        },
    },
    math::FloatOrd,
    prelude::*,
    render::{
        mesh::PrimitiveTopology,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AsBindGroup, BindGroup, BindGroupLayout,
            BlendState, Buffer, BufferInitDescriptor, BufferUsages,
            ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState,
            FragmentState, FrontFace, IndexFormat, MultisampleState, PipelineCache, PolygonMode,
            PrimitiveState, RawBufferVec, RenderPipelineDescriptor,
            SpecializedRenderPipeline, SpecializedRenderPipelines, StencilFaceState, StencilState,
            TextureFormat, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
            VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        storage::GpuShaderStorageBuffer,
        sync_world::{MainEntity, RenderEntity, SyncToRenderWorld},
        texture::{FallbackImage, GpuImage},
        view::RenderVisibleEntities,
        Extract, Render, RenderApp, RenderSet,
    },
    sprite::{Mesh2dPipeline, SetMesh2dViewBindGroup},
};

use bytemuck::{Pod, Zeroable};

pub struct MyRenderPlugin;
impl Plugin for MyRenderPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .add_render_command::<Transparent2d, MyDrawCommand>()
            .init_resource::<SpecializedRenderPipelines<CustomPipeline>>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(
                Render,
                (
                    queue.in_set(RenderSet::Queue),
                    prepare.in_set(RenderSet::PrepareBindGroups),
                ),
            );
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app.init_resource::<CustomPipeline>();
        render_app.init_resource::<FixedQuadMesh>();
    }
}

// -------------------
// My Data
#[derive(AsBindGroup, Component, Clone)]
#[require(SyncToRenderWorld)]
pub struct CustomSprite {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}

#[derive(Component)]
struct ExtractedSpriteInstance {
    instance_data: SpriteTransformMatrix,
    z_order: f32,
}

#[derive(Resource)]
pub struct FixedQuadMesh {
    vertex_buffer: RawBufferVec<Vec3>,
    index_buffer: RawBufferVec<u32>,
}

impl FromWorld for FixedQuadMesh {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let render_queue = world.resource::<RenderQueue>();

        let mut vertex_buffer = RawBufferVec::<Vec3>::new(BufferUsages::VERTEX);
        let mut index_buffer = RawBufferVec::<u32>::new(BufferUsages::INDEX);

        vertex_buffer.extend([
            Vec3::new(0., 0., 0.),
            Vec3::new(1., 0., 0.),
            Vec3::new(1., 1., 0.),
            Vec3::new(0., 1., 0.),
        ]);
        vertex_buffer.write_buffer(render_device, render_queue);

        index_buffer.extend([
            0, 1, 2, // first triangle
            0, 2, 3, // second triangle
        ]);

        index_buffer.write_buffer(render_device, render_queue);

        Self {
            vertex_buffer,
            index_buffer,
        }
    }
}

// -------------------
// extract
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct SpriteTransformMatrix([Vec4; 3]);

impl From<&GlobalTransform> for SpriteTransformMatrix {
    fn from(value: &GlobalTransform) -> Self {
        let transposed_transform_3x3 = value
            .compute_transform()
            .compute_affine()
            .matrix3
            .transpose();

        Self([
            transposed_transform_3x3
                .x_axis
                .extend(value.translation().x),
            transposed_transform_3x3
                .y_axis
                .extend(value.translation().y),
            transposed_transform_3x3
                .z_axis
                .extend(value.translation().z),
        ])
    }
}

/// copy data from the game world into the render world
/// RUNS IN RENDER WORLD
fn extract(
    mut commands: Commands,
    sprites: Extract<
        Query<(
            RenderEntity,
            &GlobalTransform,
            &ViewVisibility,
            &CustomSprite,
        )>,
    >,
) {
    if sprites.is_empty() {
        warn!("No sprites found in extract");
        return;
    }

    for (render_entity, transform, visibilty, sprite) in sprites.iter() {
        if !visibilty.get() {
            warn!("Sprite is not visible x");
            continue;
        }

        commands.entity(render_entity).insert((
            ExtractedSpriteInstance {
                instance_data: SpriteTransformMatrix::from(transform),
                z_order: transform.translation().z,
            },
            sprite.clone(),
        ));
    }
}

// QUEUE: get all the camera/views, and add the appropriate items to that view's render phase.
//        ie, just stuff visible to that view.
//        (remember each view/camera has a render phase comp that holds phase items)
// ON RENDER WORLD
fn queue(
    transparent_2d_draw_functions: Res<DrawFunctions<Transparent2d>>,
    my_pipeline: Res<CustomPipeline>, // failed
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<CustomPipeline>>,
    mut render_phases: ResMut<ViewSortedRenderPhases<Transparent2d>>,
    visible_entities: Query<(Entity, &RenderVisibleEntities)>,
    extracted_sprites: Query<(Entity, &MainEntity, &ExtractedSpriteInstance)>,
) {
    let my_draw_function = transparent_2d_draw_functions.read().id::<MyDrawCommand>();

    // iterate over each camera
    for (view_entity, view_visible_entities) in visible_entities.iter() {
        let Some(render_phase) = render_phases.get_mut(&view_entity) else {
            info!("no render phase found for camera");
            continue;
        };
        // load the pipline from the loaded pipeline cache
        let key = CustomPipelineKey;
        let pipeline = pipelines.specialize(&pipeline_cache, &my_pipeline, key);

        if extracted_sprites.is_empty() {
            warn!("No extracted sprites found");
            continue;
        }

        for (render_entity, main_entity, sprite) in extracted_sprites.iter() {
            //check if the current camera can see our entity
            if !view_visible_entities
                .get::<With<CustomSprite>>()
                .contains(&(render_entity, *main_entity))
            {
                warn!("Camera cannot see entity");
                continue;
            }

            // add a `PhaseItem` for our entity to the cameras render phase
            render_phase.add(Transparent2d {
                sort_key: FloatOrd(sprite.z_order),
                entity: (render_entity, *main_entity),
                pipeline,
                draw_function: my_draw_function,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::NONE,
            })
        }
    }
}

// -------------------
// prepared buffers, ready to be passed to the gpu
#[derive(Component)]
pub struct PreparedSprites {
    uniform_buffer: BindGroup,
    instance_buffer: Buffer,
    count: u32,
}

// transform our data into a wgpu buffer and prepare it for binding in the final draw command
fn prepare(
    mut cmd: Commands,
    render_device: Res<RenderDevice>,
    images: Res<RenderAssets<GpuImage>>,
    shader_storage_buffer: Res<RenderAssets<GpuShaderStorageBuffer>>,
    fallback_image: Res<FallbackImage>,
    pipeline: Res<CustomPipeline>,
    extracted_sprites: Query<(Entity, &ExtractedSpriteInstance, &CustomSprite)>,
) {
    let mut param = (images, fallback_image, shader_storage_buffer);
    for (entity, sprite_instance, custom_sprite) in extracted_sprites.iter() {
        let instance_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("transform buffer"),
            contents: bytemuck::cast_slice(&[sprite_instance.instance_data]),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });

        let Ok(uniform_buffer) =
            custom_sprite.as_bind_group(&pipeline.uniform_layout, &render_device, &mut param)
        else {
            continue;
        };

        cmd.entity(entity).insert(PreparedSprites {
            uniform_buffer: uniform_buffer.bind_group,
            instance_buffer,
            count: 1,
        });
    }
}

// -------------------
// Pipeline
#[derive(Resource)]
pub struct CustomPipeline {
    view_layout: BindGroupLayout,
    uniform_layout: BindGroupLayout,
    shader: Handle<Shader>,
}

#[derive(PartialEq, Eq, Clone, Hash)]
pub struct CustomPipelineKey;

impl FromWorld for CustomPipeline {
    fn from_world(world: &mut World) -> Self {
        let server = world.resource::<AssetServer>();
        let render_device = world.resource::<RenderDevice>();

        // let tonemapping_lut_entries = get_lut_bind_group_layout_entries();
        // let view_layout = render_device.create_bind_group_layout(
        //     "mesh_2d_view_layout",
        //     &BindGroupLayoutEntries::sequential(
        //         ShaderStages::VERTEX_FRAGMENT,
        //         (
        //             uniform_buffer::<ViewUniform>(true),
        //             uniform_buffer::<GlobalsUniform>(false),
        //             tonemapping_lut_entries[0].visibility(ShaderStages::FRAGMENT),
        //             tonemapping_lut_entries[1].visibility(ShaderStages::FRAGMENT),
        //         ),
        //     ),
        // );
        
        // copy view layout from the mesh 2d pipeline.
        // this adds the view and globals uniform buffer bindings
        let mesh_pipeline = world.resource::<Mesh2dPipeline>();

        let view_layout = mesh_pipeline.view_layout.clone();
        let uniform_layout = CustomSprite::bind_group_layout(render_device);

        Self {
            view_layout,
            uniform_layout,
            shader: server.load("shader.wgsl"),
        }
    }
}

impl SpecializedRenderPipeline for CustomPipeline {
    type Key = CustomPipelineKey;

    #[rustfmt::skip]
    fn specialize(&self, _key: Self::Key) -> RenderPipelineDescriptor { 
        let shader_defs = vec![
            // #[cfg(feature = "webgl")]
            "SIXTEEN_BYTE_ALIGNMENT".into(),
        ];
        // TODO feels wrong to have to copy and paste in a bunch of defaults here,
        //      can i clone them from the Transparent2d pipeline somehow?
        RenderPipelineDescriptor {
            label: Some("my pipeline".into()),
            layout: vec![
                self.view_layout.clone(),
                self.uniform_layout.clone()
            ],
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: vec![],
                entry_point: "vertex".into(),
                buffers: vec![
                    // vertex buffer
                    VertexBufferLayout{
                        array_stride: 12,
                        step_mode: VertexStepMode::Vertex,
                        attributes: vec![
                            VertexAttribute{
                                format: VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0
                            }
                        ]
                    },
                    // instance buffer
                    VertexBufferLayout {
                    array_stride: 48,
                    step_mode: VertexStepMode::Instance,
                    attributes: vec![
                        // translation
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 1,
                        },
                        // rotation
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 2,
                        },
                        // scale
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 3,
                        },
                    ],
                }],
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs,
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba8UnormSrgb,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                front_face: FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
            },
            push_constant_ranges: vec![],
            // TODO: copied from the 2d pipeline, but can't we clone it from somewhere without copy paste, so it doesn't rot?
            depth_stencil: Some(DepthStencilState {
                format: CORE_2D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Always,
                stencil: StencilState {
                    front: StencilFaceState::IGNORE,
                    back: StencilFaceState::IGNORE,
                    read_mask: 0,
                    write_mask: 0,
                },
                bias: DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            }),
            multisample: MultisampleState{
                count: 4,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            zero_initialize_workgroup_memory: true,
        }
    }
}

// -------------------
// Draw command
type MyDrawCommand = (
    // ready our pipline to start sending commands
    SetItemPipeline,
    // bind the camera view uniform
    SetMesh2dViewBindGroup<0>,
    // bind our custom uniform
    SetBindGroup<1>,
    // finally send the draw command
    DrawSprite,
);

// copy our uniform buffer to the gpu
pub struct SetBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<PreparedSprites>;

    #[inline]
    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, Self::ViewQuery>,
        prepared_data: Option<ROQueryItem<'w, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(prepared_sprite) = prepared_data else {
            // this happens once. i suppose it doesn't matter, so skip not failure.
            // return RenderCommandResult::Failure("missing prepared sprite");
            return RenderCommandResult::Skip;
        };

        // bind our texture
        pass.set_bind_group(I, &prepared_sprite.uniform_buffer, &[]);

        RenderCommandResult::Success
    }
}

// -----------------------------
// send the draw command
pub struct DrawSprite;
impl<P: PhaseItem> RenderCommand<P> for DrawSprite {
    type Param = SRes<FixedQuadMesh>;
    type ViewQuery = ();
    type ItemQuery = Read<PreparedSprites>;

    #[inline]
    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, Self::ViewQuery>,
        prepared_data: Option<ROQueryItem<'w, Self::ItemQuery>>,
        param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(prepared_sprite) = prepared_data else {
            return RenderCommandResult::Failure("missing prepared sprite");
        };

        let quad_data = param.into_inner();

        let Some(index_buffer) = quad_data.index_buffer.buffer() else {
            return RenderCommandResult::Failure("missing index buffer");
        };

        let Some(vertex_buffer) = quad_data.vertex_buffer.buffer() else {
            return RenderCommandResult::Failure("missing vertex buffer");
        };

        // pass the vertex buffer
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));

        // pass the instance buffer
        pass.set_vertex_buffer(1, prepared_sprite.instance_buffer.slice(..));

        // pass the index buffer
        pass.set_index_buffer(index_buffer.slice(..), 0, IndexFormat::Uint32);

        // finally draw
        pass.draw_indexed(0..6, 0, 0..prepared_sprite.count);

        RenderCommandResult::Success
    }
}
