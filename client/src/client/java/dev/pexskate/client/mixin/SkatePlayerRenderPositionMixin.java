package dev.pexskate.client.mixin;

import dev.pexskate.client.SkatePoseCache;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.entity.LivingEntityRenderer;
import net.minecraft.client.renderer.entity.state.LivingEntityRenderState;
import net.minecraft.world.entity.LivingEntity;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Places active skaters at the same sampled simulation root used by their camera and skeleton. */
@Mixin(LivingEntityRenderer.class)
public abstract class SkatePlayerRenderPositionMixin {
    @Inject(
            method = "extractRenderState(Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/client/renderer/entity/state/LivingEntityRenderState;F)V",
            at = @At("TAIL")
    )
    private void pexskate$alignSkaterRenderPosition(LivingEntity entity,
                                                    LivingEntityRenderState state,
                                                    float partialTick,
                                                    CallbackInfo ci) {
        Minecraft client = Minecraft.getInstance();
        boolean localEntity = client.player != null && client.player.getId() == entity.getId();
        SkatePoseCache.Pose pose = SkatePoseCache.sample(entity.getId(), localEntity);
        if (pose == null) return;

        float[] root = pose.rootPosition();
        state.x = root[0];
        state.y = root[1];
        state.z = root[2];
    }
}
