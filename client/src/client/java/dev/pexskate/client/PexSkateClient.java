package dev.pexskate.client;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.keymapping.v1.KeyMappingHelper;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.minecraft.client.CameraType;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.core.component.DataComponents;
import net.minecraft.world.entity.Display.ItemDisplay;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.resources.Identifier;
import net.minecraft.network.chat.Component;
import java.util.HashMap;
import java.util.Map;

public final class PexSkateClient implements ClientModInitializer {
    private static final KeyMapping.Category CATEGORY = KeyMapping.Category.register(
            Identifier.fromNamespaceAndPath("pexskate", "controls")
    );
    private static final KeyMapping TOGGLE = KeyMappingHelper.registerKeyMapping(new KeyMapping(
            "key.pexskate.toggle",
            InputConstants.Type.KEYBOARD,
            InputConstants.KEY_G,
            CATEGORY
    ));
    private static final KeyMapping FLICK_LEFT = flickKey("key.pexskate.flick_left", InputConstants.KEY_LEFT);
    private static final KeyMapping FLICK_RIGHT = flickKey("key.pexskate.flick_right", InputConstants.KEY_RIGHT);
    private static final KeyMapping FLICK_UP = flickKey("key.pexskate.flick_up", InputConstants.KEY_UP);
    private static final KeyMapping FLICK_DOWN = flickKey("key.pexskate.flick_down", InputConstants.KEY_DOWN);

    private boolean skating;
    private CameraType previousCamera;
    private static final Map<Integer, ItemDisplay> BOARDS = new HashMap<>();
    private static final Map<Integer, Long> LAST_SEEN = new HashMap<>();
    private static final Identifier BOARD_MODEL = Identifier.fromNamespaceAndPath("pexskate", "skateboard");

    private static KeyMapping flickKey(String translationKey, int keyCode) {
        return KeyMappingHelper.registerKeyMapping(new KeyMapping(
                translationKey, InputConstants.Type.KEYBOARD, keyCode, CATEGORY));
    }

    @Override
    public void onInitializeClient() {
        PayloadTypeRegistry.serverboundPlay().register(ControlPayload.TYPE, ControlPayload.CODEC);
        PayloadTypeRegistry.clientboundPlay().register(SkateStatePayload.TYPE, SkateStatePayload.CODEC);
        ClientPlayNetworking.registerGlobalReceiver(SkateStatePayload.TYPE, (payload, context) -> {
            if (payload.version() == 4) {
                context.client().execute(() -> {
                    SkatePoseCache.accept(payload);
                    updateBoard(context.client(), payload);
                });
            }
        });
        ClientPlayConnectionEvents.DISCONNECT.register((handler, client) -> {
            clearBoards();
            if (skating && previousCamera != null) {
                client.options.setCameraType(previousCamera);
            }
            skating = false;
            previousCamera = null;
        });
        ClientTickEvents.END_CLIENT_TICK.register(this::tick);
    }

    private void updateBoard(Minecraft client, SkateStatePayload state) {
        syncLocalCamera(client, state);
        ItemDisplay board = BOARDS.get(state.entityId());
        if (!state.active()) {
            if (board != null) board.discard();
            BOARDS.remove(state.entityId());
            LAST_SEEN.remove(state.entityId());
            SkatePoseCache.remove(state.entityId());
            return;
        }
        if (client.level == null) return;
        if (board == null || board.level() != client.level) {
            if (board != null) board.discard();
            board = EntityTypes.ITEM_DISPLAY.create(client.level, EntitySpawnReason.TRIGGERED);
            if (board == null) return;
            board.setId(-1_000_000 - state.entityId());
            board.setNoGravity(true);
            ItemStack boardItem = new ItemStack(Items.OAK_PRESSURE_PLATE);
            boardItem.set(DataComponents.ITEM_MODEL, BOARD_MODEL);
            board.setItemStack(boardItem);
            client.level.addEntity(board);
            BOARDS.put(state.entityId(), board);
        }
        board.setPos(state.boardPosX(), state.boardPosY() + 0.03, state.boardPosZ());
        // Network order is x, y, z, w; entity yaw/pitch provide the closest
        // vanilla ItemDisplay orientation while preserving the full pose for
        // clients that replace the display renderer.
        float bx = state.boardRotX();
        float by = state.boardRotY();
        float bz = state.boardRotZ();
        float bw = state.boardRotW();
        float yaw = (float) Math.toDegrees(Math.atan2(2.0f * (bw * by + bx * bz), 1.0f - 2.0f * (by * by + bz * bz)));
        float pitch = (float) Math.toDegrees(Math.asin(Math.clamp(2.0f * (bw * bx - bz * by), -1.0f, 1.0f)));
        board.setYRot(yaw);
        board.setXRot(pitch);
        LAST_SEEN.put(state.entityId(), client.level.getGameTime());

        if (client.player != null && client.player.getId() == state.entityId()) {
            String label = state.trickName().isBlank()
                    ? String.format("Speed %.1f m/s   Line %.0f", Math.sqrt(state.velocityX() * state.velocityX() + state.velocityZ() * state.velocityZ()), state.lineScore())
                    : String.format("%s   %.0f x%.1f%s", state.trickName(), state.sequenceScore(), state.multiplier(),
                            (state.scoreFlags() & 1) != 0 ? "  CLEAN" : (state.scoreFlags() & 2) != 0 ? "  SKETCHY" : "");
            client.player.sendOverlayMessage(Component.literal(label));
        }
    }

    private void syncLocalCamera(Minecraft client, SkateStatePayload state) {
        if (client.player == null || client.player.getId() != state.entityId() || skating == state.active()) {
            return;
        }
        skating = state.active();
        if (skating) {
            previousCamera = client.options.getCameraType();
            client.options.setCameraType(CameraType.THIRD_PERSON_BACK);
        } else if (previousCamera != null) {
            client.options.setCameraType(previousCamera);
            previousCamera = null;
        }
    }

    private static void clearBoards() {
        BOARDS.values().forEach(ItemDisplay::discard);
        BOARDS.clear();
        LAST_SEEN.clear();
        SkatePoseCache.clear();
    }

    private void tick(Minecraft client) {
        while (TOGGLE.consumeClick()) {
            if (client.player == null) {
                continue;
            }
            client.player.connection.sendCommand("skate");
        }

        if (!skating || client.player == null || client.getConnection() == null) {
            clearStaleBoards(client);
            return;
        }
        clearStaleBoards(client);

        int flags = 0;
        if (client.options.keySprint.isDown() || client.options.keyUp.isDown() || client.options.keyJump.isDown()) flags |= 1;
        if (client.options.keyShift.isDown()) flags |= 2;
        if (client.options.keyDown.isDown()) flags |= 4;

        int forward = 0;
        int steer = (client.options.keyRight.isDown() ? 127 : 0)
                - (client.options.keyLeft.isDown() ? 127 : 0);
        int rightX = (FLICK_RIGHT.isDown() ? 127 : 0) - (FLICK_LEFT.isDown() ? 127 : 0);
        int rightY = (FLICK_UP.isDown() ? 127 : 0) - (FLICK_DOWN.isDown() ? 127 : 0);

        ClientPlayNetworking.send(new ControlPayload(
                (byte) 2, (byte) flags, (byte) forward, (byte) steer, (byte) rightX, (byte) rightY));
    }

    private static void clearStaleBoards(Minecraft client) {
        if (client.level == null) {
            return;
        }
        long now = client.level.getGameTime();
        LAST_SEEN.entrySet().removeIf(entry -> {
            if (now - entry.getValue() <= 100) {
                return false;
            }
            ItemDisplay board = BOARDS.remove(entry.getKey());
            if (board != null) board.discard();
            SkatePoseCache.remove(entry.getKey());
            return true;
        });
    }
}
