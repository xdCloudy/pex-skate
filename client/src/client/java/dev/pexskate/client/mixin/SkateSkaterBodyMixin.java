package dev.pexskate.client.mixin;

import dev.pexskate.client.SkatePoseCache;
import dev.pexskate.client.SkateSkaterModel;
import net.minecraft.client.renderer.entity.LivingEntityRenderer;
import net.minecraft.client.renderer.entity.state.AvatarRenderState;
import net.minecraft.client.renderer.entity.state.LivingEntityRenderState;
import net.minecraft.client.renderer.rendertype.RenderType;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Suppresses the vanilla avatar body only when a locally loaded retail mesh will replace it. */
@Mixin(LivingEntityRenderer.class)
public abstract class SkateSkaterBodyMixin {
    @Inject(
            method = "getRenderType(Lnet/minecraft/client/renderer/entity/state/LivingEntityRenderState;ZZZ)Lnet/minecraft/client/renderer/rendertype/RenderType;",
            at = @At("HEAD"),
            cancellable = true
    )
    private void pexskate$hideVanillaBody(LivingEntityRenderState state, boolean bodyVisible,
                                          boolean translucent, boolean glowing,
                                          CallbackInfoReturnable<RenderType> cir) {
        if (state instanceof AvatarRenderState avatar && SkateSkaterModel.isReady()
                && SkatePoseCache.sample(avatar.id) != null) {
            cir.setReturnValue(null);
        }
    }
}
