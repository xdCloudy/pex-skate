package dev.pexskate.client.mixin;

import dev.pexskate.client.SkatePoseCache;
import net.minecraft.client.renderer.culling.Frustum;
import net.minecraft.client.renderer.entity.EntityRenderDispatcher;
import net.minecraft.world.entity.Entity;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Keep active skaters visible when Minecraft's custom-camera frustum rejects their entity box. */
@Mixin(EntityRenderDispatcher.class)
public abstract class SkateEntityVisibilityMixin {
    @Inject(method = "shouldRender(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/client/renderer/culling/Frustum;DDDF)Z",
            at = @At("HEAD"), cancellable = true)
    private <E extends Entity> void pexskate$keepSkaterVisible(E entity, Frustum frustum,
                                                                double x, double y, double z,
                                                                float tickDelta,
                                                                CallbackInfoReturnable<Boolean> callback) {
        if (SkatePoseCache.sample(entity.getId()) != null) callback.setReturnValue(true);
    }
}
