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
import net.minecraft.world.entity.Display.ItemDisplay;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.resources.Identifier;
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

    private boolean skating;
    private CameraType previousCamera;
    private static final Map<Integer, ItemDisplay> BOARDS = new HashMap<>();
    private static final Map<Integer, Long> LAST_SEEN = new HashMap<>();

    @Override
    public void onInitializeClient() {
        PayloadTypeRegistry.serverboundPlay().register(ControlPayload.TYPE, ControlPayload.CODEC);
        PayloadTypeRegistry.clientboundPlay().register(SkateStatePayload.TYPE, SkateStatePayload.CODEC);
        ClientPlayNetworking.registerGlobalReceiver(SkateStatePayload.TYPE, (payload, context) -> {
            if (payload.version() == 1) {
                context.client().execute(() -> updateBoard(context.client(), payload));
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

    private static void updateBoard(Minecraft client, SkateStatePayload state) {
        ItemDisplay board = BOARDS.get(state.entityId());
        if (!state.active()) {
            if (board != null) board.discard();
            BOARDS.remove(state.entityId());
            LAST_SEEN.remove(state.entityId());
            return;
        }
        if (client.level == null) return;
        if (board == null || board.level() != client.level) {
            if (board != null) board.discard();
            board = EntityTypes.ITEM_DISPLAY.create(client.level, EntitySpawnReason.TRIGGERED);
            if (board == null) return;
            board.setId(-1_000_000 - state.entityId());
            board.setNoGravity(true);
            board.setItemStack(new ItemStack(Items.OAK_PRESSURE_PLATE));
            client.level.addEntity(board);
            BOARDS.put(state.entityId(), board);
        }
        board.setPos(state.x(), state.y() + 0.03, state.z());
        board.setYRot(state.yaw());
        board.setXRot((float) Math.toDegrees(state.roll()));
        LAST_SEEN.put(state.entityId(), client.level.getGameTime());
    }

    private static void clearBoards() {
        BOARDS.values().forEach(ItemDisplay::discard);
        BOARDS.clear();
        LAST_SEEN.clear();
    }

    private void tick(Minecraft client) {
        while (TOGGLE.consumeClick()) {
            if (client.player == null) {
                continue;
            }
            skating = !skating;
            client.player.connection.sendCommand("skate");
            if (skating) {
                previousCamera = client.options.getCameraType();
                client.options.setCameraType(CameraType.THIRD_PERSON_BACK);
            } else if (previousCamera != null) {
                client.options.setCameraType(previousCamera);
                previousCamera = null;
            }
        }

        if (!skating || client.player == null || client.getConnection() == null) {
            clearStaleBoards(client);
            return;
        }
        clearStaleBoards(client);

        int flags = 0;
        if (client.options.keySprint.isDown()) flags |= 1;
        if (client.options.keyShift.isDown()) flags |= 2;
        if (client.options.keyJump.isDown()) flags |= 4;
        if (client.options.keyAttack.isDown()) flags |= 8;
        if (client.options.keyUse.isDown()) flags |= 16;

        int forward = (client.options.keyUp.isDown() ? 127 : 0)
                - (client.options.keyDown.isDown() ? 127 : 0);
        int steer = (client.options.keyLeft.isDown() ? 127 : 0)
                - (client.options.keyRight.isDown() ? 127 : 0);

        ClientPlayNetworking.send(new ControlPayload((byte) 1, (byte) flags, (byte) forward, (byte) steer));
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
            return true;
        });
    }
}
