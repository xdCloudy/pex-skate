package dev.pexskate.client;

import com.mojang.blaze3d.platform.InputConstants;
import com.mojang.math.Transformation;
import org.lwjgl.sdl.SDLGamepad;
import org.lwjgl.sdl.SDLInit;
import org.lwjgl.sdl.SDLStdinc;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientLifecycleEvents;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.keymapping.v1.KeyMappingHelper;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.client.rendering.v1.LivingEntityRenderLayerRegistrationCallback;
import net.minecraft.client.CameraType;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.client.model.player.PlayerModel;
import net.minecraft.client.renderer.entity.state.AvatarRenderState;
import net.minecraft.core.component.DataComponents;
import net.minecraft.world.entity.Display.ItemDisplay;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.client.renderer.entity.RenderLayerParent;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.resources.Identifier;
import net.minecraft.network.chat.Component;
import java.util.HashMap;
import java.util.Map;
import java.nio.IntBuffer;

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
    private static final KeyMapping PUSH_RIGHT = flickKey("key.pexskate.push_right", InputConstants.KEY_W);
    private static final KeyMapping PUSH_LEFT = flickKey("key.pexskate.push_left", InputConstants.KEY_LSHIFT);
    private static final KeyMapping BRAKE = flickKey("key.pexskate.brake", InputConstants.KEY_S);
    private static final KeyMapping STEER_LEFT = flickKey("key.pexskate.steer_left", InputConstants.KEY_A);
    private static final KeyMapping STEER_RIGHT = flickKey("key.pexskate.steer_right", InputConstants.KEY_D);
    private static final KeyMapping FLICK_LEFT = flickKey("key.pexskate.flick_left", InputConstants.KEY_LEFT);
    private static final KeyMapping FLICK_RIGHT = flickKey("key.pexskate.flick_right", InputConstants.KEY_RIGHT);
    private static final KeyMapping FLICK_UP = flickKey("key.pexskate.flick_up", InputConstants.KEY_UP);
    private static final KeyMapping FLICK_DOWN = flickKey("key.pexskate.flick_down", InputConstants.KEY_DOWN);
    private static final KeyMapping GRAB_LEFT = flickKey("key.pexskate.grab_left", InputConstants.KEY_Q);
    private static final KeyMapping GRAB_RIGHT = flickKey("key.pexskate.grab_right", InputConstants.KEY_E);
    private static final KeyMapping DPAD_UP = flickKey("key.pexskate.dpad_up", InputConstants.KEY_I);
    private static final KeyMapping DPAD_DOWN = flickKey("key.pexskate.dpad_down", InputConstants.KEY_K);
    private static final KeyMapping DPAD_LEFT = flickKey("key.pexskate.dpad_left", InputConstants.KEY_U);
    private static final KeyMapping DPAD_RIGHT = flickKey("key.pexskate.dpad_right", InputConstants.KEY_O);
    private static final KeyMapping DISMOUNT = flickKey("key.pexskate.dismount", InputConstants.KEY_F);
    private static final KeyMapping SESSION_MODIFIER = flickKey("key.pexskate.session_modifier", InputConstants.KEY_LCONTROL);
    private static final KeyMapping RESPAWN = flickKey("key.pexskate.respawn", InputConstants.KEY_R);

    private static volatile boolean skateModeActive;
    private boolean skating;
    private CameraType previousCamera;
    private long gamepadHandle;
    private boolean gamepadSubsystemReady;
    private static final Map<Integer, ItemDisplay> BOARDS = new HashMap<>();
    private static final Map<Integer, Long> LAST_SEEN = new HashMap<>();
    private static final Identifier BOARD_MODEL = Identifier.fromNamespaceAndPath("pexskate", "skateboard");
    private static boolean mapInstallWarningShown;

    private static KeyMapping flickKey(String translationKey, int keyCode) {
        return KeyMappingHelper.registerKeyMapping(new KeyMapping(
                translationKey, InputConstants.Type.KEYBOARD, keyCode, CATEGORY));
    }

    @Override
    public void onInitializeClient() {
        gamepadSubsystemReady = SDLInit.SDL_InitSubSystem(SDLInit.SDL_INIT_GAMEPAD);
        PayloadTypeRegistry.serverboundPlay().register(ClientHelloPayload.TYPE, ClientHelloPayload.CODEC);
        PayloadTypeRegistry.serverboundPlay().register(ControlPayload.TYPE, ControlPayload.CODEC);
        PayloadTypeRegistry.clientboundPlay().register(SkateStatePayload.TYPE, SkateStatePayload.CODEC);
        ClientPlayNetworking.registerGlobalReceiver(SkateStatePayload.TYPE, (payload, context) -> {
            if (payload.version() == 4 || payload.version() == 5 || payload.version() == 6 || payload.version() == 7) {
                context.client().execute(() -> {
                    if (payload.active() && payload.mapBacked()
                            && !SkateMapModel.isReadyFor(payload.mapFilename())) {
                        if (payload.version() >= 7) SkateMapModel.loadMap(payload.mapFilename());
                        else SkateMapModel.loadDefault();
                    }
                    SkatePoseCache.accept(payload);
                    SkateMapModel.acceptState(payload.entityId(), payload.active(), payload.mapBacked());
                    updateBoard(context.client(), payload);
                });
            }
        });
        SkateMapModel.registerRenderer();
        LivingEntityRenderLayerRegistrationCallback.EVENT.register((entityType, renderer, helper, context) -> {
            if (entityType == EntityTypes.PLAYER) {
                @SuppressWarnings("unchecked")
                RenderLayerParent<AvatarRenderState, PlayerModel> parent =
                        (RenderLayerParent<AvatarRenderState, PlayerModel>) renderer;
                helper.register(new SkateSkaterRenderLayer(parent));
            }
        });
        ClientLifecycleEvents.CLIENT_STARTED.register(client -> {
            SkateSkaterModel.load();
        });
        ClientPlayConnectionEvents.JOIN.register((handler, sender, client) ->
                ClientPlayNetworking.send(new ClientHelloPayload((byte) 3)));
        ClientPlayConnectionEvents.DISCONNECT.register((handler, client) -> {
            clearBoards();
            SkateMapModel.clearActive();
            mapInstallWarningShown = false;
            if (skating && previousCamera != null) {
                client.options.setCameraType(previousCamera);
            }
            skating = false;
            skateModeActive = false;
            previousCamera = null;
        });
        ClientLifecycleEvents.CLIENT_STOPPING.register(client -> {
            SkateSkaterModel.close();
            SkateMapModel.close();
            if (gamepadHandle != 0L) {
                SDLGamepad.SDL_CloseGamepad(gamepadHandle);
                gamepadHandle = 0L;
            }
            if (gamepadSubsystemReady) {
                SDLInit.SDL_QuitSubSystem(SDLInit.SDL_INIT_GAMEPAD);
                gamepadSubsystemReady = false;
            }
        });
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            SkateMapModel.tick(client);
            tick(client);
        });
    }

    private void updateBoard(Minecraft client, SkateStatePayload state) {
        syncLocalCamera(client, state);
        if (client.player != null && client.player.getId() == state.entityId()) {
            if (state.active() && state.mapBacked()
                    && !SkateMapModel.isReadyFor(state.mapFilename())
                    && !SkateMapModel.isLoadingFor(state.mapFilename()) && !mapInstallWarningShown) {
                client.player.sendOverlayMessage(Component.literal(
                        "Install the matching converted Skate map in config/pex-skate/maps to render this district."));
                mapInstallWarningShown = true;
            } else if (!state.active()) {
                mapInstallWarningShown = false;
            }
        }
        ItemDisplay board = BOARDS.get(state.entityId());
        if (!state.active()) {
            if (board != null) board.discard();
            BOARDS.remove(state.entityId());
            LAST_SEEN.remove(state.entityId());
            SkatePoseCache.remove(state.entityId());
            return;
        }
        if (client.level == null) return;
        if (SkateSkaterModel.isReady()) {
            // The converted retail GLB includes the deck, trucks, and wheels,
            // all skinned to the Skate engine's board joints. Keep the
            // pressure-plate display only as the no-assets fallback.
            if (board != null) board.discard();
            BOARDS.remove(state.entityId());
        } else {
            if (board == null || board.level() != client.level) {
                if (board != null) board.discard();
                board = EntityTypes.ITEM_DISPLAY.create(client.level, EntitySpawnReason.TRIGGERED);
                if (board == null) return;
                board.setId(-1_000_000 - state.entityId());
                board.setNoGravity(true);
                board.setTransformationInterpolationDuration(3);
                board.setPosRotInterpolationDuration(3);
                ItemStack boardItem = new ItemStack(Items.OAK_PRESSURE_PLATE);
                boardItem.set(DataComponents.ITEM_MODEL, BOARD_MODEL);
                board.setItemStack(boardItem);
                client.level.addEntity(board);
                BOARDS.put(state.entityId(), board);
            }
            board.setPos(state.boardPosX(), state.boardPosY() + 0.03, state.boardPosZ());
            // Apply the complete engine orientation so flips and board roll
            // survive the server/client bridge instead of collapsing to yaw.
            float bx = state.boardRotX();
            float by = state.boardRotY();
            float bz = state.boardRotZ();
            float bw = state.boardRotW();
            float rotationLength = (float) Math.sqrt(bx * bx + by * by + bz * bz + bw * bw);
            if (Float.isFinite(rotationLength) && rotationLength > 1.0e-5f) {
                board.setTransformation(new Transformation(
                        new org.joml.Vector3f(),
                        new org.joml.Quaternionf(bx, by, bz, bw).normalize(),
                        new org.joml.Vector3f(1.0f),
                        new org.joml.Quaternionf()));
            }
        }
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
        skateModeActive = skating;
        if (skating) {
            previousCamera = client.options.getCameraType();
            client.options.setCameraType(CameraType.THIRD_PERSON_BACK);
        } else if (previousCamera != null) {
            client.options.setCameraType(previousCamera);
            previousCamera = null;
        }
    }

    public static boolean isSkateModeActive() {
        return skateModeActive;
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

        Gamepad pad = readGamepad();
        int flags = 0;
        if (PUSH_RIGHT.isDown() || client.options.keyJump.isDown() || pad.pushRight()) flags |= 1;
        if (PUSH_LEFT.isDown() || pad.pushLeft()) flags |= 2;
        if (BRAKE.isDown() || pad.brake()) flags |= 4;
        if (DISMOUNT.isDown() || pad.dismount()) flags |= 0x80;

        float steer = clamp(keyAxis(STEER_RIGHT.isDown(), STEER_LEFT.isDown()) + pad.leftX());
        float rightX = clamp(keyAxis(FLICK_RIGHT.isDown(), FLICK_LEFT.isDown()) + pad.rightX());
        float rightY = clamp(keyAxis(FLICK_UP.isDown(), FLICK_DOWN.isDown()) + pad.rightY());
        float leftTrigger = Math.max(pad.leftTrigger(), GRAB_LEFT.isDown() ? 1.0f : 0.0f);
        float rightTrigger = Math.max(pad.rightTrigger(), GRAB_RIGHT.isDown() ? 1.0f : 0.0f);
        int dpad = pad.dpad();
        if (DPAD_UP.isDown()) dpad |= 1;
        if (DPAD_DOWN.isDown()) dpad |= 2;
        if (DPAD_LEFT.isDown()) dpad |= 4;
        if (DPAD_RIGHT.isDown()) dpad |= 8;
        flags |= dpad << 3;
        boolean respawn = RESPAWN.isDown() || (pad.sessionModifier() && pad.pushLeft());
        byte modifiers = (byte) ((SESSION_MODIFIER.isDown() || pad.sessionModifier() ? 1 : 0)
                | (respawn ? 2 : 0));

        ClientPlayNetworking.send(new ControlPayload(
                (byte) 7, (byte) flags,
                quantizeTrigger(leftTrigger), quantizeTrigger(rightTrigger),
                quantize(pad.forward()), quantize(steer), quantize(rightX), quantize(rightY),
                modifiers));
    }

    private Gamepad readGamepad() {
        if (!gamepadSubsystemReady) return Gamepad.NONE;

        if (gamepadHandle != 0L && !SDLGamepad.SDL_GamepadConnected(gamepadHandle)) {
            SDLGamepad.SDL_CloseGamepad(gamepadHandle);
            gamepadHandle = 0L;
        }
        if (gamepadHandle == 0L) {
            if (!SDLGamepad.SDL_HasGamepad()) return Gamepad.NONE;
            IntBuffer gamepads = SDLGamepad.SDL_GetGamepads();
            if (gamepads != null) {
                try {
                    for (int i = 0; i < gamepads.remaining(); i++) {
                        gamepadHandle = SDLGamepad.SDL_OpenGamepad(gamepads.get(gamepads.position() + i));
                        if (gamepadHandle != 0L) break;
                    }
                } finally {
                    SDLStdinc.SDL_free(gamepads);
                }
            }
        }
        if (gamepadHandle == 0L) return Gamepad.NONE;

        return new Gamepad(
                deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_LEFTX))),
                -deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_LEFTY))),
                deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_RIGHTX))),
                -deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_RIGHTY))),
                trigger(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_LEFT_TRIGGER)),
                trigger(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_RIGHT_TRIGGER)),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_SOUTH),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_WEST),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_EAST),
                dpadButtons(),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_NORTH),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_LEFT_SHOULDER));
    }

    private int dpadButtons() {
        int result = 0;
        if (SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_DPAD_UP)) result |= 1;
        if (SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_DPAD_DOWN)) result |= 2;
        if (SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_DPAD_LEFT)) result |= 4;
        if (SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_DPAD_RIGHT)) result |= 8;
        return result;
    }

    private static float axis(short value) {
        return Math.max(-1.0f, value / 32767.0f);
    }

    private static float trigger(short value) {
        return Math.clamp(value / 32767.0f, 0.0f, 1.0f);
    }

    private static float keyAxis(boolean positive, boolean negative) {
        return (positive ? 1.0f : 0.0f) - (negative ? 1.0f : 0.0f);
    }

    private static float deadzone(float value) {
        float magnitude = Math.abs(value);
        if (!Float.isFinite(value) || magnitude <= 0.15f) return 0.0f;
        return Math.copySign(Math.min(1.0f, (magnitude - 0.15f) / 0.85f), value);
    }

    private static float clamp(float value) {
        return Math.clamp(value, -1.0f, 1.0f);
    }

    private static byte quantize(float value) {
        return (byte) Math.round(clamp(value) * 127.0f);
    }

    private static byte quantizeTrigger(float value) {
        return (byte) Math.round(Math.clamp(value, 0.0f, 1.0f) * 255.0f);
    }

    private record Gamepad(float leftX, float forward, float rightX, float rightY,
                           float leftTrigger, float rightTrigger,
                           boolean pushRight, boolean pushLeft, boolean brake, int dpad,
                           boolean dismount, boolean sessionModifier) {
        private static final Gamepad NONE = new Gamepad(
                0.0f, 0.0f, 0.0f, 0.0f, 0.0f, 0.0f, false, false, false, 0, false, false);
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
