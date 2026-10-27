/* Reproduce pinned Java 1.21.11 canonical block physics without a server/world.
 * JDK21 arguments: <runtime-root> <output.json> <this-source.java>.
 * Compile against the pinned inner server jar and its official library jars;
 * run nativephysics.JavaBlockPhysics with -XX:ActiveProcessorCount=1 -Xmx1G
 * -Djava.security.manager=allow. Output is native-state-physics-v4, directly
 * consumable by the repository's block-physics importer after metadata cleanup.
 */
package nativephysics;

import com.google.gson.Gson;
import java.lang.reflect.*;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;

public final class JavaBlockPhysics {
    static final String MC = "net.minecraft.";
    static final String BASE = MC + "world.level.block.state.BlockBehaviour$BlockStateBase";
    static final String POS = MC + "core.BlockPos", DIR = MC + "core.Direction";
    static final String GETTER = MC + "world.level.BlockGetter", BLOCK = MC + "world.level.block.Block";
    static final String VOXEL = MC + "world.phys.shapes.VoxelShape", WALL = MC + "world.level.block.WallBlock";
    static final String SERVER = "64bb6d763bed0a9f1d632ec347938594144943ed";
    static final String MAPPINGS = "5621e9253f05fd57872bbe7f8ddf5f9a7d525955";
    static final String INNER = "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc";
    final Map<String, String> classes = new HashMap<>();
    final Map<String, Map<String, String>> members = new HashMap<>();

    JavaBlockPhysics(Path mappings) throws Exception {
        String owner = null;
        for (String line : Files.readAllLines(mappings)) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] parts = line.trim().split(" -> ", 2);
            if (parts.length != 2) continue;
            if (!Character.isWhitespace(line.charAt(0))) {
                owner = parts[0]; classes.put(owner, parts[1].substring(0, parts[1].length() - 1));
                members.put(owner, new HashMap<>());
            } else if (owner != null) {
                String signature = parts[0].replaceFirst("^\\d+:\\d+:", "");
                int open = signature.indexOf('(');
                String key = open < 0 ? signature.substring(signature.lastIndexOf(' ') + 1)
                    : signature.substring(signature.lastIndexOf(' ', open) + 1, signature.indexOf(')', open) + 1);
                members.get(owner).putIfAbsent(key, parts[1]);
            }
        }
    }
    Class<?> type(String name) throws Exception {
        return switch (name) { case "int" -> int.class; default -> Class.forName(classes.getOrDefault(name, name)); };
    }
    Method method(String owner, String name, String... parameters) throws Exception {
        String mapped = Objects.requireNonNull(members.get(owner).get(name + "(" + String.join(",", parameters) + ")"), owner + "." + name);
        Class<?>[] types = new Class<?>[parameters.length];
        for (int i = 0; i < types.length; ++i) types[i] = type(parameters[i]);
        Method result;
        try { result = type(owner).getMethod(mapped, types); }
        catch (NoSuchMethodException error) { result = type(owner).getDeclaredMethod(mapped, types); }
        result.setAccessible(true); return result;
    }
    Object call(String owner, String name, Object receiver, String[] parameters, Object... args) throws Exception {
        return method(owner, name, parameters).invoke(receiver, args);
    }
    Object field(String owner, String name) throws Exception {
        Field result = type(owner).getDeclaredField(Objects.requireNonNull(members.get(owner).get(name), owner + "." + name));
        result.setAccessible(true); return result.get(null);
    }
    static String hash(Path path, String algorithm) throws Exception {
        MessageDigest digest = MessageDigest.getInstance(algorithm);
        try (var stream = Files.newInputStream(path)) {
            byte[] bytes = new byte[65536]; int count;
            while ((count = stream.read(bytes)) != -1) digest.update(bytes, 0, count);
        }
        return HexFormat.of().formatHex(digest.digest());
    }
    static void pin(Path path, String expected, String algorithm) throws Exception {
        String actual = hash(path, algorithm);
        if (!actual.equals(expected)) throw new IllegalArgumentException("unexpected native artifact hash: " + actual);
    }
    void loadVanillaTags() throws Exception {
        Object access = call(MC + "core.RegistryAccess", "fromRegistryOfRegistries", null,
            new String[]{MC + "core.Registry"}, field(MC + "core.registries.BuiltInRegistries", "REGISTRY"));
        Object pack = call(MC + "server.packs.repository.ServerPacksSource", "createVanillaPackSource", null, new String[]{});
        Object resources = type(MC + "server.packs.resources.MultiPackResourceManager")
            .getConstructor(type(MC + "server.packs.PackType"), List.class)
            .newInstance(field(MC + "server.packs.PackType", "SERVER_DATA"), List.of(pack));
        try {
            List<?> pending = (List<?>) call(MC + "tags.TagLoader", "loadTagsForExistingRegistries", null,
                new String[]{MC + "server.packs.resources.ResourceManager", MC + "core.RegistryAccess"}, resources, access);
            for (Object tags : pending) call(MC + "core.Registry$PendingTags", "apply", tags, new String[]{});
        } finally {
            call(MC + "server.packs.resources.MultiPackResourceManager", "close", resources, new String[]{});
        }
    }
    List<Object> states() throws Exception {
        Object getter = field(MC + "world.level.EmptyBlockGetter", "INSTANCE"), zero = field(POS, "ZERO");
        Object registry = field(BLOCK, "BLOCK_STATE_REGISTRY"), blocks = field(MC + "core.registries.BuiltInRegistries", "BLOCK");
        Method id = method(MC + "core.IdMapper", "getId", "java.lang.Object");
        Method block = method(BASE, "getBlock"), blockName = method(MC + "core.Registry", "getKey", "java.lang.Object");
        Method values = method(MC + "world.level.block.state.StateHolder", "getValues");
        String property = MC + "world.level.block.state.properties.Property";
        Method propertyName = method(property, "getName"), propertyValue = method(property, "getName", "java.lang.Comparable");
        Method sturdy = method(BASE, "isFaceSturdy", GETTER, POS, DIR);
        Method centerSturdy = method(BASE, "isFaceSturdy", GETTER, POS, DIR, MC + "world.level.block.SupportType");
        Object center = field(MC + "world.level.block.SupportType", "CENTER");
        Method exception = method(BLOCK, "isExceptionForConnection", MC + "world.level.block.state.BlockState");
        Method motion = method(BASE, "blocksMotion"), solid = method(BASE, "isSolid"), render = method(BASE, "isSolidRender");
        Method full = method(BASE, "isCollisionShapeFullBlock", GETTER, POS), replaceable = method(BASE, "canBeReplaced");
        Method collision = method(BASE, "getCollisionShape", GETTER, POS), face = method(VOXEL, "getFaceShape", DIR);
        Method faceFull = method(BLOCK, "isFaceFull", VOXEL, DIR), covered = method(WALL, "isCovered", VOXEL, VOXEL);
        List<Object> directions = new ArrayList<>();
        for (String name : List.of("DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST")) directions.add(field(DIR, name));
        List<Object> wallTests = new ArrayList<>(); wallTests.add(field(WALL, "TEST_SHAPE_POST"));
        Map<?, ?> sides = (Map<?, ?>) field(WALL, "TEST_SHAPES_WALL");
        for (int direction : new int[]{2, 5, 3, 4}) wallTests.add(sides.get(directions.get(direction)));
        List<Object> rows = new ArrayList<>();
        for (Object state : (Iterable<?>) registry) {
            int support = 0, centerFlags = 0, wallFlags = 0, behavior = 0;
            for (int direction = 0; direction < 6; ++direction) {
                if ((boolean) sturdy.invoke(state, getter, zero, directions.get(direction))) support |= 1 << direction;
                if ((boolean) centerSturdy.invoke(state, getter, zero, directions.get(direction), center)) centerFlags |= 1 << direction;
            }
            if ((boolean) exception.invoke(null, state)) support |= 64;
            if ((boolean) motion.invoke(state)) support |= 128;
            Object shape = collision.invoke(state, getter, zero), down = face.invoke(shape, directions.get(0));
            for (int bit = 0; bit < wallTests.size(); ++bit) if ((boolean) covered.invoke(null, down, wallTests.get(bit))) wallFlags |= 1 << bit;
            if ((boolean) solid.invoke(state)) behavior |= 1;
            if ((boolean) render.invoke(state)) behavior |= 2;
            if ((boolean) full.invoke(state, getter, zero)) behavior |= 4;
            if ((boolean) replaceable.invoke(state)) behavior |= 8;
            if ((boolean) faceFull.invoke(null, shape, directions.get(1))) behavior |= 16;
            TreeMap<String, String> properties = new TreeMap<>();
            for (var entry : ((Map<?, ?>) values.invoke(state)).entrySet()) properties.put(
                (String) propertyName.invoke(entry.getKey()), (String) propertyValue.invoke(entry.getKey(), entry.getValue()));
            List<Object> pairs = new ArrayList<>(); properties.forEach((key, value) -> pairs.add(List.of(key, value)));
            rows.add(Map.of("id", id.invoke(registry, state), "block", blockName.invoke(blocks, block.invoke(state)).toString(),
                "properties", pairs, "flags", support, "wall_above_flags", wallFlags, "center_support_flags", centerFlags, "behavior_flags", behavior));
        }
        return rows;
    }
    @SuppressWarnings("removal")
    public static void main(String[] args) throws Exception {
        if (args.length != 3) throw new IllegalArgumentException("<runtime-root> <output.json> <JavaBlockPhysics.java>");
        Path root = Path.of(args[0]);
        pin(root.resolve("server.jar"), SERVER, "SHA-1"); pin(root.resolve("server-mappings.txt"), MAPPINGS, "SHA-1");
        pin(root.resolve("versions/1.21.11/server-1.21.11.jar"), INNER, "SHA-256");
        System.setSecurityManager(new SecurityManager() {
            public void checkPermission(java.security.Permission permission) {}
            public void checkConnect(String host, int port) { throw new SecurityException("network forbidden"); }
            public void checkListen(int port) { throw new SecurityException("listeners forbidden"); }
            public void checkAccept(String host, int port) { throw new SecurityException("network forbidden"); }
        });
        var factory = (org.apache.logging.log4j.core.impl.Log4jContextFactory) org.apache.logging.log4j.LogManager.getFactory();
        var context = factory.getSelector().getContext("JavaBlockPhysics", ClassLoader.getSystemClassLoader(), false);
        var logging = new org.apache.logging.log4j.core.config.DefaultConfiguration();
        ((Map<String, String>) logging.getComponent("ContextProperties")).put("hostName", "localhost"); context.start(logging);
        JavaBlockPhysics query = new JavaBlockPhysics(root.resolve("server-mappings.txt"));
        query.call(MC + "SharedConstants", "tryDetectVersion", null, new String[]{});
        query.call(MC + "server.Bootstrap", "bootStrap", null, new String[]{});
        query.loadVanillaTags();
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("schema", "native-state-physics-v4"); result.put("version", "1.21.11"); result.put("server_sha1", SERVER);
        result.put("mappings_sha1", MAPPINGS); result.put("inner_sha256", INNER); result.put("source_sha256", hash(Path.of(args[2]), "SHA-256"));
        result.put("states", query.states());
        Files.writeString(Path.of(args[1]), new Gson().toJson(result));
        System.exit(0);
    }
}
