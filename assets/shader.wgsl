#import bevy_render::{ maths::affine3_to_square, view::View, globals::Globals }

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> globals: Globals;

@group(1) @binding(0) var texture: texture_2d<f32>;
@group(1) @binding(1) var texture_sampler: sampler;

struct VertexInput{
    @builtin(vertex_index) index: u32,

	// vertex buffer
	@location(0) position : vec3<f32>,

	// instance buffer
    @location(1) i_translation: vec4<f32>,
    @location(2) i_rotation: vec4<f32>,
    @location(3) i_scale: vec4<f32>,
}


struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
	@location(1) uv : vec2<f32>,
};


@vertex
fn vertex(in: VertexInput) -> VertexOutput{
	var out : VertexOutput;

	// vertices
	// 0,1  1,1
	// X-----X
	// |    /|
	// |   / |
	// |  /  |
	// | /   |
	// |/    |
	// X-----X
	// 0,0   1,0

	// compute the instance matrix
	let instance_transform = affine3_to_square(mat3x4<f32>(
        in.i_translation,
        in.i_rotation,
        in.i_scale,
    ));

	// read the texture size
	let size = vec2<f32>(textureDimensions(texture));

	// some simple vertex animation
	let animation_scale = vec2(1., max(abs(sin(globals.time)), 0.2));

	// offset the vertex, so our quad center matches the transform
	let offset_position = in.position - vec3(0.5, 0.5, 0.);

	// multiple the vertex by the projection matrix and the instance transform
    out.clip_position =
		view.clip_from_world
		* instance_transform
		* vec4<f32>(offset_position * vec3(size * animation_scale,1.), 1.0);

	// inverse y axis for uv map
	out.uv = vec2(in.position.x, 1. - in.position.y);

	return out;
}


@fragment
fn fragment(in : VertexOutput) -> @location(0) vec4<f32> {
	return textureSample(texture, texture_sampler, in.uv);
}
