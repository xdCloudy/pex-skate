package dev.pexskate.client;

import com.mojang.blaze3d.platform.InputConstants;
import com.mojang.math.Transformation;
import org.lwjgl.sdl.SDLGamepad;
import org.lwjgl.sdl.SDLInit;
import org.lwjgl.system.MemoryUtil;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientLifecycleEvents;
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
    private static final KeyMapping FLICK_LEFT = flickKey("key.pexskate.flick_left", InputConstants.KEY_LEFT);
    private static final KeyMapping FLICK_RIGHT = flickKey("key.pexskate.flick_right", InputConstants.KEY_RIGHT);
    private static final KeyMapping FLICK_UP = flickKey("key.pexskate.flick_up", InputConstants.KEY_UP);
    private static final KeyMapping FLICK_DOWN = flickKey("key.pexskate.flick_down", InputConstants.KEY_DOWN);

    private boolean skating;
    private CameraType previousCamera;
    private long gamepadHandle;
    private boolean gamepadSubsystemReady;
    private static final Map<Integer, ItemDisplay> BOARDS = new HashMap<>();
    private static final Map<Integer, Long> LAST_SEEN = new HashMap<>();
    private static final Identifier BOARD_MODEL = Identifier.fromNamespaceAndPath("pexskate", "skateboard");

    private static KeyMapping flickKey(String translationKey, int keyCode) {
        return KeyMappingHelper.registerKeyMapping(new KeyMapping(
                translationKey, InputConstants.Type.KEYBOARD, keyCode, CATEGORY));
    }

    @Override
    public void onInitializeClient() {
        gamepadSubsystemReady = SDLInit.SDL_InitSubSystem(SDLInit.SDL_INIT_GAMEPAD);
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
        ClientLifecycleEvents.CLIENT_STOPPING.register(client -> {
            if (gamepadHandle != 0L) {
                SDLGamepad.SDL_CloseGamepad(gamepadHandle);
                gamepadHandle = 0L;
            }
            if (gamepadSubsystemReady) {
                SDLInit.SDL_QuitSubSystem(SDLInit.SDL_INIT_GAMEPAD);
                gamepadSubsystemReady = false;
            }
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
            board.setTransformationInterpolationDuration(3);
            board.setPosRotInterpolationDuration(3);
            ItemStack boardItem = new ItemStack(Items.OAK_PRESSURE_PLATE);
            boardItem.set(DataComponents.ITEM_MODEL, BOARD_MODEL);
            board.setItemStack(boardItem);
            client.level.addEntity(board);
            BOARDS.put(state.entityId(), board);
        }
        board.setPos(state.boardPosX(), state.boardPosY() + 0.03, state.boardPosZ());
        // Apply the complete engine orientation so flips and board roll survive
        // the server/client bridge instead of collapsing to yaw and pitch.
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

        Gamepad pad = readGamepad();
        int flags = 0;
        if (client.options.keyUp.isDown() || client.options.keyJump.isDown()
                || pad.pushRight()) flags |= 1;
        if (client.options.keyShift.isDown() || pad.pushLeft()) flags |= 2;
        if (client.options.keyDown.isDown() || pad.brake()) flags |= 4;

        float steer = clamp(keyAxis(client.options.keyRight.isDown(), client.options.keyLeft.isDown()) + pad.leftX());
        float rightX = clamp(keyAxis(FLICK_RIGHT.isDown(), FLICK_LEFT.isDown()) + pad.rightX());
        float rightY = clamp(keyAxis(FLICK_UP.isDown(), FLICK_DOWN.isDown()) + pad.rightY());

        ClientPlayNetworking.send(new ControlPayload(
                (byte) 2, (byte) flags, quantize(pad.forward()), quantize(steer), quantize(rightX), quantize(rightY)));
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
                    MemoryUtil.memFree(gamepads);
                }
            }
        }
        if (gamepadHandle == 0L) return Gamepad.NONE;

        return new Gamepad(
                deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_LEFTX))),
                -deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_LEFTY))),
                deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_RIGHTX))),
                -deadzone(axis(SDLGamepad.SDL_GetGamepadAxis(gamepadHandle, SDLGamepad.SDL_GAMEPAD_AXIS_RIGHTY))),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_SOUTH),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_WEST),
                SDLGamepad.SDL_GetGamepadButton(gamepadHandle, SDLGamepad.SDL_GAMEPAD_BUTTON_EAST));
    }

    private static float axis(short value) {
        return Math.max(-1.0f, value / 32767.0f);
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

    private record Gamepad(float leftX, float forward, float rightX, float rightY,
                           boolean pushRight, boolean pushLeft, boolean brake) {
        private static final Gamepad NONE = new Gamepad(0.0f, 0.0f, 0.0f, 0.0f, false, false, false);
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
