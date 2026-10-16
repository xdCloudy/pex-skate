package dev.pexskate.client;

import com.mojang.blaze3d.vertex.PoseStack;
import net.minecraft.client.model.player.PlayerModel;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.entity.RenderLayerParent;
import net.minecraft.client.renderer.entity.layers.RenderLayer;
import net.minecraft.client.renderer.entity.LivingEntityRenderer;
import net.minecraft.client.renderer.entity.state.AvatarRenderState;

/** Draws the locally converted retail skater over the matching server pose. */
final class SkateSkaterRenderLayer extends RenderLayer<AvatarRenderState, PlayerModel> {
    SkateSkaterRenderLayer(RenderLayerParent<AvatarRenderState, PlayerModel> parent) {
        super(parent);
    }

    @Override
    public void submit(PoseStack matrices, SubmitNodeCollector collector, int light,
                       AvatarRenderState state, float limbSwing, float limbSwingAmount) {
        if (state.isInvisible || state.isInvisibleToPlayer || !SkateSkaterModel.isReady()) return;
        SkatePoseCache.Pose pose = SkatePoseCache.sample(state.id);
        if (pose == null) return;
        SkateSkaterModel.submit(matrices, collector, light,
                LivingEntityRenderer.getOverlayCoords(state, 0.0f), pose);
    }
}
