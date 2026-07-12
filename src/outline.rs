//! Outline 后处理 plugin：给像素世界不透明物体加黑色描边。
//!
//! 对应原作 anchor 的 outline.frag（5×5 邻域 alpha 膨胀）。
//! 挂在 [`OutlinePostProcess`] 标记的相机上，对其输出跑一个全屏后处理 pass。
//! 结构照搬本地 bevy/examples/shader_advanced/custom_post_processing.rs（v0.19.0），改成 2D、去掉 settings uniform。

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{Core2dSystems, FullscreenShader, schedule::Core2d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            ColorTargetState, ColorWrites, FilterMode, FragmentState, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, TextureFormat, TextureSampleType, TextureViewId,
            binding_types::{sampler, texture_2d},
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::ViewTarget,
    },
    shader::Shader,
};

const OUTLINE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("b335ceb4-5b64-4719-91c3-64e60aa8bea4");

/// 挂在相机上，标记该相机的输出走 outline 后处理。
#[derive(Component, Default, Clone, Copy, ExtractComponent)]
pub struct OutlinePostProcess;

pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        // Mini Game packagers filter unknown extensions such as `.wgsl`, so
        // the render pipeline must not depend on a runtime shader file.
        load_internal_asset!(
            app,
            OUTLINE_SHADER_HANDLE,
            "shaders/outline.wgsl",
            Shader::from_wgsl
        );

        // 把主世界的 OutlinePostProcess 标记抽到渲染世界，决定哪些 view 跑后处理。
        app.add_plugins(ExtractComponentPlugin::<OutlinePostProcess>::default());

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init_outline_pipeline);
        render_app.add_systems(
            Core2d,
            outline_post_process.in_set(Core2dSystems::PostProcess),
        );
    }
}

#[derive(Resource)]
struct OutlinePipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
}

/// `ViewTarget::post_process_write()` 在 A/B 两张主纹理之间轮换。
///
/// 只缓存一个 bind group 会在每次纹理翻转时都重新创建 GPU 对象；WebGPU
/// 后端通常能很快回收，但微信 WebGL 桥会让这些原生对象持续堆积。游戏有 5
/// 个描边相机、每个相机 2 张 A/B 纹理，因此这里保留一个有界的 16 槽缓存，
/// 同时覆盖所有稳定目标，并在分辨率变化时轮换淘汰旧槽。
struct BoundedCache<K, V, const N: usize> {
    entries: [Option<(K, V)>; N],
    next: usize,
}

impl<K, V, const N: usize> Default for BoundedCache<K, V, N> {
    fn default() -> Self {
        assert!(N > 0, "bounded cache must have at least one slot");
        Self {
            entries: std::array::from_fn(|_| None),
            next: 0,
        }
    }
}

impl<K: PartialEq, V, const N: usize> BoundedCache<K, V, N> {
    fn get_or_insert_with(&mut self, key: K, create: impl FnOnce() -> V) -> &V {
        if let Some(index) = self.entries.iter().position(|entry| {
            entry
                .as_ref()
                .is_some_and(|(cached_key, _)| cached_key == &key)
        }) {
            return &self.entries[index].as_ref().expect("cache entry exists").1;
        }

        let index = self.next;
        self.next = (self.next + 1) % N;
        self.entries[index] = Some((key, create()));
        &self.entries[index]
            .as_ref()
            .expect("cache entry was inserted")
            .1
    }
}

type OutlineBindGroupCache = BoundedCache<TextureViewId, BindGroup, 16>;

fn init_outline_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    // 只有 source texture + sampler 两个 binding（outline 无配置 uniform）。
    let layout = BindGroupLayoutDescriptor::new(
        "outline_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );

    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });

    commands.insert_resource(OutlinePipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
    commands.insert_resource(SpecializedRenderPipelines::<OutlinePipeline>::default());
}

impl SpecializedRenderPipeline for OutlinePipeline {
    type Key = TextureFormat;

    fn specialize(&self, target_format: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("outline_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: OUTLINE_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

fn outline_post_process(
    view: ViewQuery<(&ViewTarget, &OutlinePostProcess)>,
    pipeline: Option<Res<OutlinePipeline>>,
    pipelines: Option<ResMut<SpecializedRenderPipelines<OutlinePipeline>>>,
    pipeline_cache: Res<PipelineCache>,
    mut cache: Local<OutlineBindGroupCache>,
    mut ctx: RenderContext,
) {
    let (Some(pipeline), Some(mut pipelines)) = (pipeline, pipelines) else {
        return;
    };
    let (view_target, _marker) = view.into_inner();

    let pipeline_id = pipelines.specialize(
        &pipeline_cache,
        &pipeline,
        view_target.main_texture_format(),
    );
    let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline_id) else {
        return;
    };

    // 拿到后处理的 source/destination（ViewTarget 主纹理翻转）。
    let post_process = view_target.post_process_write();

    let layout = pipeline_cache.get_bind_group_layout(&pipeline.layout);
    let bind_group = cache.get_or_insert_with(post_process.source.id(), || {
        ctx.render_device().create_bind_group(
            "outline_bind_group",
            &layout,
            &BindGroupEntries::sequential((post_process.source, &pipeline.sampler)),
        )
    });

    let mut render_pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("outline_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

    render_pass.set_pipeline(render_pipeline);
    render_pass.set_bind_group(0, bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::BoundedCache;

    #[test]
    fn alternating_post_process_targets_are_created_once_each() {
        let mut cache = BoundedCache::<_, _, 2>::default();
        let mut creates = 0;

        for key in [1, 2, 1, 2, 1, 2] {
            let value = cache.get_or_insert_with(key, || {
                creates += 1;
                key * 10
            });
            assert_eq!(*value, key * 10);
        }

        assert_eq!(creates, 2);
    }

    #[test]
    fn resized_targets_replace_old_slots_without_growing_the_cache() {
        let mut cache = BoundedCache::<_, _, 2>::default();
        for key in [1, 2, 3, 4] {
            cache.get_or_insert_with(key, || key);
        }

        assert_eq!(cache.entries.iter().flatten().count(), 2);
        assert!(cache.entries.iter().flatten().any(|(key, _)| *key == 3));
        assert!(cache.entries.iter().flatten().any(|(key, _)| *key == 4));
    }
}
