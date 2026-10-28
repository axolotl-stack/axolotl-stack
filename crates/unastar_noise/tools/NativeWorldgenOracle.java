/* Reference-only probes of the pinned Java 1.21.11 public API.
 * Uses official Mojang mappings supplied outside the repository; starts no
 * server, opens no sockets, and never invokes the game/server entrypoint.
 * Run with JDK21 and the bundled server classpath, capped to one processor.
 * Coordinates use colon separators. column-constructor-fields observes freshly
 * constructed native one-cell column contexts, not public-call instrumentation.
 * configured-carver takes an explicit registry ID before chunk coordinates;
 * its complete canonical volumes are primitives, never CARVERS-stage references.
 * fullstage uses dimension argument "all" and the repository's pinned
 * NativeContextProbe/NativeContextProbeSnapshot helper sources plus
 * -Djava.security.manager=allow. It captures real SURFACE/CARVERS/FEATURES and
 * separately labeled native FEATURES halo closure through constructor-only
 * native server contexts, without initServer/runServer, listeners, or ticks.
 * Its optional --structures=true|false coordinate-prefix flag defaults to false
 * and controls actual native WorldOptions, not only capture metadata.
 * --trace-native-tasks after the structures flag records actual native pyramid
 * dependencies and delegating task before/future-completion events; it does not
 * replace native scheduling or await a different future.
 * Compile nativefullprobe/Launcher.java alongside all three probe sources and
 * launch nativefullprobe.Launcher to isolate only our default-package probes
 * from the original signed native classes. Fullstage creates a fresh world
 * directory per invocation. --native-village-anchors after --structures=true
 * optionally appends a 3x3 capture grid around a real native village lookup.
 * aquifer-trace extends the real fillFromNoise passive observer with a delegating
 * Aquifer interface observer: consumed density/result are actual calls; center,
 * fluid and pressure diagnostics are labeled native reflective post-call probes.
 * feature-order reads the actual generated preset's memoized featuresPerStep
 * supplier and native StepFeatureData index mapping using tagged vanilla
 * registries. It needs no coordinates, world creation, or feature placement.
 * Optional -Dunastar.native.surface.biomeWitnesses=x:y:z[,x:y:z...] observes
 * actual returned native surface source-biome holders. It separately labels
 * post-pass native BiomeManager fuzzy lookups and their matching in-pass
 * winning-quart callbacks; ordinary captures do not enable this observer.
 */
import com.google.gson.Gson;
import java.io.BufferedReader;
import java.lang.reflect.Array;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.*;
import java.util.stream.Stream;

public final class NativeWorldgenOracle {
    private static final String SERVER_SHA1 = "64bb6d763bed0a9f1d632ec347938594144943ed";
    private static final String INNER_SHA256 = "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc";
    private static final String MAPPINGS_SHA1 = "5621e9253f05fd57872bbe7f8ddf5f9a7d525955";
    private static final String MC = "net.minecraft.";
    private static final String CLIMATE = MC + "world.level.biome.Climate$";
    private static final String CHUNK = MC + "world.level.chunk.";
    private static final String KEY = MC + "resources.ResourceKey";
    private static final String PARAMS = MC + "world.level.biome.MultiNoiseBiomeSourceParameterList";
    private static final String SETTINGS = MC + "world.level.levelgen.NoiseGeneratorSettings";
    private static final String RANDOM_STATE = MC + "world.level.levelgen.RandomState";
    private static final String LEVELGEN = MC + "world.level.levelgen.";
    private static final String DENSITY = LEVELGEN + "DensityFunction";
    private static final String SOURCE = MC + "world.level.biome.BiomeSource";
    private static final String ROUTER = LEVELGEN + "NoiseRouter";
    private static final String GENERATOR = LEVELGEN + "NoiseBasedChunkGenerator";
    private static final String BLENDER = LEVELGEN + "blending.Blender";
    private static final String[] ROUTER_FIELDS = {
        "barrierNoise", "fluidLevelFloodednessNoise", "fluidLevelSpreadNoise", "lavaNoise",
        "temperature", "vegetation", "continents", "erosion", "depth", "ridges",
        "preliminarySurfaceLevel", "finalDensity", "veinToggle", "veinRidged", "veinGap"
    };

    private final Map<String, String> classes = new HashMap<>();
    private final Map<String, Map<String, String>> members = new HashMap<>();

    private NativeWorldgenOracle(Path mappings) throws Exception {
        try (BufferedReader reader = Files.newBufferedReader(mappings)) {
            String owner = null;
            String line;
            while ((line = reader.readLine()) != null) {
                if (line.startsWith("#") || line.isBlank()) continue;
                if (!Character.isWhitespace(line.charAt(0))) {
                    String[] parts = line.split(" -> ", 2);
                    owner = parts[0];
                    classes.put(owner, parts[1].substring(0, parts[1].length() - 1));
                    members.put(owner, new HashMap<>());
                } else if (owner != null) {
                    String[] parts = line.trim().split(" -> ", 2);
                    if (parts.length != 2) continue;
                    String signature = parts[0].replaceFirst("^\\d+:\\d+:", "");
                    int open = signature.indexOf('(');
                    String name;
                    if (open >= 0) {
                        int close = signature.indexOf(')', open);
                        name = signature.substring(signature.lastIndexOf(' ', open) + 1, close + 1);
                    } else {
                        name = signature.substring(signature.lastIndexOf(' ') + 1);
                    }
                    members.get(owner).putIfAbsent(name, parts[1]);
                }
            }
        }
    }

    private Class<?> type(String name) throws Exception {
        if (name.endsWith("[]")) return Array.newInstance(type(name.substring(0, name.length() - 2)), 0).getClass();
        return switch (name) {
            case "byte" -> byte.class;
            case "int" -> int.class;
            case "long" -> long.class;
            case "boolean" -> boolean.class;
            case "float" -> float.class;
            case "double" -> double.class;
            default -> Class.forName(classes.getOrDefault(name, name));
        };
    }

    private Method method(String owner, String name, String... parameters) throws Exception {
        String key = name + "(" + String.join(",", parameters) + ")";
        String mapped = Objects.requireNonNull(members.get(owner).get(key), owner + "." + key);
        Class<?>[] types = new Class<?>[parameters.length];
        for (int i = 0; i < types.length; ++i) types[i] = type(parameters[i]);
        try {
            return type(owner).getMethod(mapped, types);
        } catch (NoSuchMethodException nonPublic) {
            Method result = type(owner).getDeclaredMethod(mapped, types);
            result.setAccessible(true);
            return result;
        }
    }

    private Object call(String owner, String name, Object receiver, String[] parameters, Object... arguments) throws Exception {
        return method(owner, name, parameters).invoke(receiver, arguments);
    }

    private Object field(String owner, String name) throws Exception {
        var field = type(owner).getDeclaredField(Objects.requireNonNull(members.get(owner).get(name), owner + "." + name));
        field.setAccessible(true);
        return field.get(null);
    }

    private Object construct(String owner, String[] parameters, Object... arguments) throws Exception {
        Class<?>[] types = new Class<?>[parameters.length];
        for (int i = 0; i < types.length; ++i) types[i] = type(parameters[i]);
        return type(owner).getConstructor(types).newInstance(arguments);
    }

    private static void verify(Path path, String expected, String algorithm) throws Exception {
        MessageDigest digest = MessageDigest.getInstance(algorithm);
        try (var input = Files.newInputStream(path)) {
            byte[] buffer = new byte[65536];
            int count;
            while ((count = input.read(buffer)) != -1) digest.update(buffer, 0, count);
        }
        String actual = HexFormat.of().formatHex(digest.digest());
        if (!actual.equals(expected)) throw new IllegalArgumentException(path + ": unexpected " + algorithm + " " + actual);
    }

    private record State(Object lookup, Object random, Object sampler, Object source, Object settingsHolder,
                         Object settings, Object router, Object endIslands) {}

    private Object registry(Object lookup, String key) throws Exception {
        return call(MC + "core.HolderLookup$Provider", "lookupOrThrow", lookup,
            new String[]{KEY}, field(MC + "core.registries.Registries", key));
    }

    private Object runtimeLookup() throws Exception {
        Object access = call(MC + "core.RegistryAccess", "fromRegistryOfRegistries", null,
            new String[]{MC + "core.Registry"}, field(MC + "core.registries.BuiltInRegistries", "REGISTRY"));
        Object pack = call(MC + "server.packs.repository.ServerPacksSource", "createVanillaPackSource", null, new String[]{});
        Object resources = construct(MC + "server.packs.resources.MultiPackResourceManager",
            new String[]{MC + "server.packs.PackType", "java.util.List"},
            field(MC + "server.packs.PackType", "SERVER_DATA"), List.of(pack));
        try {
            List<?> pending = (List<?>) call(MC + "tags.TagLoader", "loadTagsForExistingRegistries", null,
                new String[]{MC + "server.packs.resources.ResourceManager", MC + "core.RegistryAccess"}, resources, access);
            for (Object tags : pending) call(MC + "core.Registry$PendingTags", "apply", tags, new String[]{});
            Stream<?> context = (Stream<?>) call(MC + "core.HolderLookup$Provider", "listRegistries", access, new String[]{});
            return call(MC + "resources.RegistryDataLoader", "load", null,
                new String[]{MC + "server.packs.resources.ResourceManager", "java.util.List", "java.util.List"},
                resources, context.toList(), field(MC + "resources.RegistryDataLoader", "WORLDGEN_REGISTRIES"));
        } finally {
            call(MC + "server.packs.resources.MultiPackResourceManager", "close", resources, new String[]{});
        }
    }

    private State bootstrap(long seed, String dimension, boolean runtimeRegistries) throws Exception {
        call(MC + "SharedConstants", "tryDetectVersion", null, new String[]{});
        call(MC + "server.Bootstrap", "bootStrap", null, new String[]{});
        Object lookup = runtimeRegistries ? runtimeLookup()
            : call(MC + "data.registries.VanillaRegistries", "createLookup", null, new String[]{});
        String preset = switch (dimension) {
            case "overworld" -> "OVERWORLD";
            case "nether" -> "NETHER";
            case "end" -> "END";
            default -> throw new IllegalArgumentException("unsupported dimension " + dimension);
        };
        Object settingsHolder = call(MC + "core.HolderGetter", "getOrThrow", registry(lookup, "NOISE_SETTINGS"),
            new String[]{KEY}, field(SETTINGS, preset));
        Object settings = call(MC + "core.Holder", "value", settingsHolder, new String[]{});
        Object random = call(RANDOM_STATE, "create", null,
            new String[]{MC + "core.HolderGetter$Provider", KEY, "long"}, lookup, field(SETTINGS, preset), seed);
        Object sampler = call(RANDOM_STATE, "sampler", random, new String[]{});
        Object source;
        if (dimension.equals("end")) {
            source = call(MC + "world.level.biome.TheEndBiomeSource", "create", null,
                new String[]{MC + "core.HolderGetter"}, registry(lookup, "BIOME"));
        } else {
            Object holder = call(MC + "core.HolderGetter", "getOrThrow",
                registry(lookup, "MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST"),
                new String[]{KEY}, field(PARAMS + "s", preset));
            source = call(MC + "world.level.biome.MultiNoiseBiomeSource", "createFromPreset", null,
                new String[]{MC + "core.Holder"}, holder);
        }
        return new State(lookup, random, sampler, source, settingsHolder, settings,
            call(RANDOM_STATE, "router", random, new String[]{}),
            call(LEVELGEN + "DensityFunctions", "endIslands", null, new String[]{"long"}, seed));
    }

    private String biome(Object source, Object sampler, int x, int y, int z) throws Exception {
        Object holder = call(SOURCE, "getNoiseBiome", source,
            new String[]{"int", "int", "int", CLIMATE + "Sampler"}, x, y, z, sampler);
        return (String) call(MC + "core.Holder", "getRegisteredName", holder, new String[]{});
    }

    private List<String> possibleBiomes(State state) throws Exception {
        Set<?> holders = (Set<?>) call(SOURCE, "possibleBiomes", state.source, new String[]{});
        List<String> names = new ArrayList<>();
        for (Object holder : holders) names.add((String) call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}));
        Collections.sort(names);
        return names;
    }

    private Method[] targetAccessors() throws Exception {
        String[] names = {"temperature", "humidity", "continentalness", "erosion", "depth", "weirdness"};
        Method[] accessors = new Method[names.length];
        for (int i = 0; i < names.length; ++i) accessors[i] = method(CLIMATE + "TargetPoint", names[i]);
        return accessors;
    }

    private static long[] targetCoordinates(Object target, Method[] accessors) throws Exception {
        long[] coordinates = new long[7];
        for (int i = 0; i < accessors.length; ++i) coordinates[i] = (long) accessors[i].invoke(target);
        return coordinates;
    }

    private Object point(State state, int[] position) throws Exception {
        Object target = call(CLIMATE + "Sampler", "sample", state.sampler,
            new String[]{"int", "int", "int"}, position[0], position[1], position[2]);
        return Map.of("quart", position, "target", targetCoordinates(target, targetAccessors()),
            "biome", biome(state.source, state.sampler, position[0], position[1], position[2]));
    }

    private String densityBits(Object density, Object context) throws Exception {
        double value = (double) call(DENSITY, "compute", density,
            new String[]{DENSITY + "$FunctionContext"}, context);
        return String.format(Locale.ROOT, "%016x", Double.doubleToRawLongBits(value));
    }

    private Object routerPoint(State state, int[] position) throws Exception {
        Object context = construct(DENSITY + "$SinglePointContext", new String[]{"int", "int", "int"},
            position[0], position[1], position[2]);
        Map<String, String> bits = new LinkedHashMap<>();
        for (String name : ROUTER_FIELDS) {
            bits.put(name, densityBits(call(ROUTER, name, state.router, new String[]{}), context));
        }
        bits.put("endIslands", densityBits(state.endIslands, context));
        return Map.of("block", position, "f64_bits", bits);
    }

    private Object biomes(State state, String dimension, List<int[]> chunks) throws Exception {
        Object sampler = state.sampler;
        Object biomeLookup = registry(state.lookup, "BIOME");
        Stream<?> entries = (Stream<?>) method(MC + "core.HolderLookup", "listElements").invoke(biomeLookup);
        List<?> holders = entries.toList();
        Object ids = construct(MC + "core.IdMapper", new String[]{});
        Method add = method(MC + "core.IdMapper", "add", "java.lang.Object");
        for (Object holder : holders) add.invoke(ids, holder);
        Object biomeStrategy = call(CHUNK + "Strategy", "createForBiomes", null, new String[]{MC + "core.IdMap"}, ids);
        Object blockStrategy = call(CHUNK + "Strategy", "createForBlockStates", null, new String[]{MC + "core.IdMap"},
            field(MC + "world.level.block.Block", "BLOCK_STATE_REGISTRY"));
        Object air = call(MC + "world.level.block.Block", "defaultBlockState",
            field(MC + "world.level.block.Blocks", "AIR"), new String[]{});
        Method sample = method(CLIMATE + "Sampler", "sample", "int", "int", "int");
        Method name = method(MC + "core.Holder", "getRegisteredName");
        Method[] targetAccessors = targetAccessors();
        Method fill = method(CHUNK + "LevelChunkSection", "fillBiomesFromNoise",
            MC + "world.level.biome.BiomeResolver", CLIMATE + "Sampler", "int", "int", "int");
        int minY = dimension.equals("overworld") ? -64 : 0;
        int height = dimension.equals("overworld") ? 384 : 256;
        List<Object> rows = new ArrayList<>();
        List<int[]> firstSectionOrder = new ArrayList<>();
        for (int[] chunk : chunks) {
            int x0 = chunk[0] * 4, z0 = chunk[1] * 4;
            String[] biomes = new String[4 * height];
            long[][] targets = new long[biomes.length][];
            Object resolver = Proxy.newProxyInstance(getClass().getClassLoader(),
                new Class<?>[]{type(MC + "world.level.biome.BiomeResolver")}, (proxy, method, args) -> {
                    if (!method.getName().equals("getNoiseBiome")) throw new UnsupportedOperationException(method.getName());
                    int x = (int) args[0], y = (int) args[1], z = (int) args[2];
                    Object target = sample.invoke(sampler, x, y, z);
                    Object holder = call(SOURCE, "getNoiseBiome", state.source,
                        new String[]{"int", "int", "int", CLIMATE + "Sampler"}, x, y, z, sampler);
                    int index = ((y - minY / 4) * 4 + z - z0) * 4 + x - x0;
                    biomes[index] = (String) name.invoke(holder);
                    targets[index] = targetCoordinates(target, targetAccessors);
                    if (firstSectionOrder.size() < 64) firstSectionOrder.add(new int[]{x, y, z});
                    return holder;
                });
            for (int y = minY / 4; y < (minY + height) / 4; y += 4) {
                Object states = construct(CHUNK + "PalettedContainer", new String[]{"java.lang.Object", CHUNK + "Strategy"}, air, blockStrategy);
                Object palette = construct(CHUNK + "PalettedContainer", new String[]{"java.lang.Object", CHUNK + "Strategy"}, holders.getFirst(), biomeStrategy);
                Object section = construct(CHUNK + "LevelChunkSection",
                    new String[]{CHUNK + "PalettedContainer", CHUNK + "PalettedContainerRO"}, states, palette);
                fill.invoke(section, resolver, sampler, x0, y, z0);
            }
            if (Arrays.stream(biomes).anyMatch(Objects::isNull)) throw new IllegalStateException("incomplete native biome volume");
            rows.add(Map.of("x", chunk[0], "z", chunk[1], "biomes", biomes, "targets", targets));
        }
        return Map.of("min_y", minY, "height", height, "chunks", rows, "first_section_query_order", firstSectionOrder,
            "query_history", "one shared native biome source; chunks in argument order; sections bottom-up; LevelChunkSection.fillBiomesFromNoise");
    }

    private Object paletteFactory(State state) throws Exception {
        Stream<?> entries = (Stream<?>) call(MC + "core.HolderLookup", "listElements",
            registry(state.lookup, "BIOME"), new String[]{});
        List<?> holders = entries.toList();
        Object ids = construct(MC + "core.IdMapper", new String[]{});
        for (Object holder : holders) call(MC + "core.IdMapper", "add", ids, new String[]{"java.lang.Object"}, holder);
        Object biomeStrategy = call(CHUNK + "Strategy", "createForBiomes", null, new String[]{MC + "core.IdMap"}, ids);
        Object blockStrategy = call(CHUNK + "Strategy", "createForBlockStates", null, new String[]{MC + "core.IdMap"},
            field(MC + "world.level.block.Block", "BLOCK_STATE_REGISTRY"));
        Object air = call(MC + "world.level.block.Block", "defaultBlockState",
            field(MC + "world.level.block.Blocks", "AIR"), new String[]{});
        Object biomeCodec = call(MC + "resources.RegistryFixedCodec", "create", null,
            new String[]{KEY}, field(MC + "core.registries.Registries", "BIOME"));
        String codec = "com.mojang.serialization.Codec";
        String[] codecParameters = {codec, CHUNK + "Strategy", "java.lang.Object"};
        Object statesCodec = call(CHUNK + "PalettedContainer", "codecRW", null, codecParameters,
            field(MC + "world.level.block.state.BlockState", "CODEC"), blockStrategy, air);
        Object biomesCodec = call(CHUNK + "PalettedContainer", "codecRO", null, codecParameters,
            biomeCodec, biomeStrategy, holders.getFirst());
        return construct(CHUNK + "PalettedContainerFactory",
            new String[]{CHUNK + "Strategy", MC + "world.level.block.state.BlockState", codec,
                CHUNK + "Strategy", MC + "core.Holder", codec},
            blockStrategy, air, statesCodec, biomeStrategy, holders.getFirst(), biomesCodec);
    }

    private String canonicalState(Object state) throws Exception {
        Object block = call(MC + "world.level.block.state.BlockBehaviour$BlockStateBase", "getBlock", state, new String[]{});
        String id = call(MC + "core.Registry", "getKey", field(MC + "core.registries.BuiltInRegistries", "BLOCK"),
            new String[]{"java.lang.Object"}, block).toString();
        Map<?, ?> values = (Map<?, ?>) call(MC + "world.level.block.state.StateHolder", "getValues", state, new String[]{});
        TreeMap<String, String> properties = new TreeMap<>();
        String property = MC + "world.level.block.state.properties.Property";
        for (var entry : values.entrySet()) {
            String name = (String) call(property, "getName", entry.getKey(), new String[]{});
            String value = (String) call(property, "getName", entry.getKey(), new String[]{"java.lang.Comparable"}, entry.getValue());
            properties.put(name, value);
        }
        if (properties.isEmpty()) return id;
        List<String> pairs = new ArrayList<>();
        properties.forEach((key, value) -> pairs.add(key + "=" + value));
        return id + "[" + String.join(",", pairs) + "]";
    }

    private static Map<String, Object> typed(String type, Object value) {
        return Map.of("type", type, "value", value);
    }

    private static Object compound(Map<String, Object> values) {
        List<Object> pairs = new ArrayList<>();
        new TreeMap<>(values).forEach((key, value) -> pairs.add(List.of(key, value)));
        return typed("Compound", pairs);
    }

    private static String dimensionId(String dimension) {
        return switch (dimension) {
            case "overworld" -> "minecraft:overworld";
            case "nether" -> "minecraft:the_nether";
            case "end" -> "minecraft:the_end";
            default -> throw new IllegalArgumentException(dimension);
        };
    }

    private static Object declaredInputs(long seed, String dimension) {
        return declaredInputs(seed, dimension, false);
    }

    private static Object declaredInputs(long seed, String dimension, boolean structures) {
        String settings = "minecraft:" + dimension;
        Object biomeSource = dimension.equals("end")
            ? compound(Map.of("type", typed("Str", "minecraft:the_end")))
            : compound(Map.of("type", typed("Str", "minecraft:multi_noise"), "preset", typed("Str", settings)));
        Object generator = compound(Map.of("type", typed("Str", "minecraft:noise"),
            "settings", typed("Str", settings), "biome_source", biomeSource));
        Object entry = compound(Map.of("type", typed("Str", dimensionId(dimension)), "generator", generator));
        return Map.of("worldgen_settings", compound(Map.of("seed", typed("Long", seed),
            "generate_features", typed("Byte", structures ? 1 : 0), "bonus_chest", typed("Byte", 0),
            "dimensions", compound(Map.of(dimensionId(dimension), entry)))),
            "enabled_features", List.of("minecraft:vanilla"), "data_profile", Map.of("kind", "vanilla"));
    }

    private record BiomeRegistry(Object registry, int entries, int tags) {}

    private BiomeRegistry loadedBiomeRegistry(State state) throws Exception {
        Object output = registry(state.lookup, "BIOME");
        if (!type(MC + "core.Registry").isInstance(output)) throw new IllegalStateException("surface requires actual runtime biome Registry");
        Stream<?> entries = (Stream<?>) call(MC + "core.HolderLookup", "listElements", output, new String[]{});
        Stream<?> tags = (Stream<?>) call(MC + "core.HolderLookup", "listTags", output, new String[]{});
        return new BiomeRegistry(output, Math.toIntExact(entries.count()), Math.toIntExact(tags.count()));
    }

    private Object surface(State state, long seed, int[] position, Object chunk, Object generator,
                           Object blender, Object bounds, Object factory, Object fluidPicker,
                           BiomeRegistry biomeRegistry) throws Exception {
        Map<String, Object> neighbors = new LinkedHashMap<>();
        List<int[]> generationOrder = new ArrayList<>();
        String center = position[0] + ":" + position[1];
        neighbors.put(center, chunk);
        generationOrder.add(position);
        for (int dz = -1; dz <= 1; ++dz) for (int dx = -1; dx <= 1; ++dx) {
            if (dx == 0 && dz == 0) continue;
            int x = position[0] + dx, z = position[1] + dz;
            Object chunkPos = construct(MC + "world.level.ChunkPos", new String[]{"int", "int"}, x, z);
            Object neighbor = construct(CHUNK + "ProtoChunk",
                new String[]{MC + "world.level.ChunkPos", CHUNK + "UpgradeData", MC + "world.level.LevelHeightAccessor",
                    CHUNK + "PalettedContainerFactory", LEVELGEN + "blending.BlendingData"},
                chunkPos, field(CHUNK + "UpgradeData", "EMPTY"), bounds, factory, null);
            Object noiseChunk = call(LEVELGEN + "NoiseChunk", "forChunk", null,
                new String[]{CHUNK + "ChunkAccess", RANDOM_STATE, LEVELGEN + "DensityFunctions$BeardifierOrMarker",
                    SETTINGS, LEVELGEN + "Aquifer$FluidPicker", BLENDER},
                neighbor, state.random, field(LEVELGEN + "Beardifier", "EMPTY"), state.settings, fluidPicker, blender);
            java.util.function.Function<Object, Object> cached = ignored -> noiseChunk;
            call(CHUNK + "ChunkAccess", "getOrCreateNoiseChunk", neighbor,
                new String[]{"java.util.function.Function"}, cached);
            var future = (java.util.concurrent.CompletableFuture<?>) call(GENERATOR, "createBiomes", generator,
                new String[]{RANDOM_STATE, BLENDER, MC + "world.level.StructureManager", CHUNK + "ChunkAccess"},
                state.random, blender, null, neighbor);
            if (future.join() != neighbor) throw new IllegalStateException("native biome fill returned another chunk");
            call(CHUNK + "ProtoChunk", "setPersistedStatus", neighbor,
                new String[]{CHUNK + "status.ChunkStatus"}, field(CHUNK + "status.ChunkStatus", "BIOMES"));
            neighbors.put(x + ":" + z, neighbor);
            generationOrder.add(new int[]{x, z});
        }
        long[] queryCount = {0};
        List<int[]> firstQueries = new ArrayList<>();
        String witnessArgument = System.getProperty("unastar.native.surface.biomeWitnesses");
        Map<String, Map<String, Object>> consumedBiomes = witnessArgument == null ? null : new LinkedHashMap<>();
        int[][] lastWinningQuart = {null};
        Object source = Proxy.newProxyInstance(getClass().getClassLoader(),
            new Class<?>[]{type(MC + "world.level.biome.BiomeManager$NoiseBiomeSource")}, (proxy, method, args) -> {
                if (!method.getName().equals("getNoiseBiome")) throw new UnsupportedOperationException(method.getName());
                int x = (int) args[0], y = (int) args[1], z = (int) args[2];
                ++queryCount[0];
                if (firstQueries.size() < 64) firstQueries.add(new int[]{x, y, z});
                String key = Math.floorDiv(x, 4) + ":" + Math.floorDiv(z, 4);
                Object neighbor = Objects.requireNonNull(neighbors.get(key), "native surface requested biome outside generated neighborhood " + key);
                Object holder = call(CHUNK + "ChunkAccess", "getNoiseBiome", neighbor,
                    new String[]{"int", "int", "int"}, x, y, z);
                if (consumedBiomes != null) {
                    lastWinningQuart[0] = new int[]{x, y, z};
                    String quartKey = x + ":" + y + ":" + z;
                    Map<String, Object> observation = consumedBiomes.get(quartKey);
                    if (observation == null) {
                        observation = new LinkedHashMap<>();
                        observation.put("quart", lastWinningQuart[0]);
                        observation.put("returned_holder_key", call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}));
                        observation.put("first_native_surface_callback_ordinal", queryCount[0]);
                        observation.put("native_surface_callback_count", 0L);
                        consumedBiomes.put(quartKey, observation);
                    }
                    observation.put("native_surface_callback_count", (long) observation.get("native_surface_callback_count") + 1);
                }
                return holder;
            });
        long zoomSeed = (long) call(MC + "world.level.biome.BiomeManager", "obfuscateSeed", null, new String[]{"long"}, seed);
        Object manager = construct(MC + "world.level.biome.BiomeManager",
            new String[]{MC + "world.level.biome.BiomeManager$NoiseBiomeSource", "long"}, source, zoomSeed);
        Object context = construct(LEVELGEN + "WorldGenerationContext",
            new String[]{CHUNK + "ChunkGenerator", MC + "world.level.LevelHeightAccessor"}, generator, bounds);
        call(GENERATOR, "buildSurface", generator,
            new String[]{CHUNK + "ChunkAccess", LEVELGEN + "WorldGenerationContext", RANDOM_STATE,
                MC + "world.level.StructureManager", MC + "world.level.biome.BiomeManager", MC + "core.Registry", BLENDER},
            chunk, context, state.random, null, manager, biomeRegistry.registry, blender);
        call(CHUNK + "ProtoChunk", "setPersistedStatus", chunk,
            new String[]{CHUNK + "status.ChunkStatus"}, field(CHUNK + "status.ChunkStatus", "SURFACE"));
        Map<String, Object> result = new LinkedHashMap<>(Map.of("x", position[0], "z", position[1], "neighbor_biome_generation_order", generationOrder,
            "neighbor_profile", "target native BIOMES then NOISE; eight neighbors native createBiomes only, z-major/x-minor; fresh neighborhood per target; shared native source across argument-order targets",
            "native_obfuscated_biome_seed", zoomSeed, "biome_manager_query_count", queryCount[0],
            "first_biome_manager_queries", firstQueries,
            "world_generation_min_y", call(LEVELGEN + "WorldGenerationContext", "getMinGenY", context, new String[]{}),
            "world_generation_height", call(LEVELGEN + "WorldGenerationContext", "getGenDepth", context, new String[]{})));
        if (consumedBiomes != null) {
            // Observe the actual manager's fuzzy choice, never substitute a center
            // quart or implement its distance predicate here. These post-pass
            // lookups are labeled separately from the consumed surface callbacks.
            Map<String, Map<String, Object>> inPass = new LinkedHashMap<>();
            for (var entry : consumedBiomes.entrySet()) inPass.put(entry.getKey(), new LinkedHashMap<>(entry.getValue()));
            List<Object> witnesses = new ArrayList<>();
            for (String coordinate : witnessArgument.split(",")) {
                int[] point = coordinates(coordinate, 3);
                if (Math.floorDiv(point[0], 16) != position[0] || Math.floorDiv(point[2], 16) != position[1]) continue;
                Object blockPos = construct(MC + "core.BlockPos", new String[]{"int", "int", "int"}, point[0], point[1], point[2]);
                lastWinningQuart[0] = null;
                Object holder = call(MC + "world.level.biome.BiomeManager", "getBiome", manager,
                    new String[]{MC + "core.BlockPos"}, blockPos);
                int[] quart = Objects.requireNonNull(lastWinningQuart[0], "native witness getter made no source callback");
                String quartKey = quart[0] + ":" + quart[1] + ":" + quart[2];
                Map<String, Object> row = new LinkedHashMap<>();
                row.put("block", point);
                row.put("native_post_pass_fuzzy_winning_quart", quart);
                row.put("native_post_pass_fuzzy_returned_holder_key", call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}));
                row.put("actual_consumed_surface_callback_for_same_winning_quart", inPass.get(quartKey));
                witnesses.add(row);
            }
            result.put("biome_witness_observer", Map.of(
                "profile", "standalone direct native surface context, not fullstage native-world context; passive in-pass source callbacks plus separately labeled actual native BiomeManager post-pass fuzzy queries; stored neighboring native chunks; no copied fuzzy predicate or source-biome resampling",
                "witnesses", witnesses));
        }
        return result;
    }

    private Object configuredCarver(State state, long seed, String id, Object generator, Object bounds,
                                    Object chunk, Object noiseChunk, Object chunkPos, int minY, int height) throws Exception {
        String carver = LEVELGEN + "carver.ConfiguredWorldCarver";
        Object identifier = call(MC + "resources.Identifier", "parse", null, new String[]{"java.lang.String"}, id);
        Object key = call(KEY, "create", null, new String[]{KEY, MC + "resources.Identifier"},
            field(MC + "core.registries.Registries", "CONFIGURED_CARVER"), identifier);
        Object holder = call(MC + "core.HolderGetter", "getOrThrow", registry(state.lookup, "CONFIGURED_CARVER"),
            new String[]{KEY}, key);
        Object configured = call(MC + "core.Holder", "value", holder, new String[]{});
        if (!type(MC + "core.RegistryAccess").isInstance(state.lookup)) {
            throw new IllegalStateException("configured-carver requires actual native runtime RegistryAccess");
        }
        Object context = construct(LEVELGEN + "carver.CarvingContext",
            new String[]{GENERATOR, MC + "core.RegistryAccess", MC + "world.level.LevelHeightAccessor",
                LEVELGEN + "NoiseChunk", RANDOM_STATE, LEVELGEN + "SurfaceRules$RuleSource"},
            generator, state.lookup, bounds, noiseChunk, state.random,
            call(SETTINGS, "surfaceRule", state.settings, new String[]{}));
        Object mask = construct(CHUNK + "CarvingMask", new String[]{"int", "int"}, height, minY);
        Object random = call(MC + "util.RandomSource", "create", null, new String[]{"long"}, seed);
        Method getState = method(CHUNK + "ProtoChunk", "getBlockState", MC + "core.BlockPos");
        int x0 = (int) call(MC + "world.level.ChunkPos", "getMinBlockX", chunkPos, new String[]{});
        int z0 = (int) call(MC + "world.level.ChunkPos", "getMinBlockZ", chunkPos, new String[]{});
        Object[] before = new Object[height * 256];
        for (int y = minY; y < minY + height; ++y) for (int z = 0; z < 16; ++z) for (int x = 0; x < 16; ++x) {
            Object pos = construct(MC + "core.BlockPos", new String[]{"int", "int", "int"}, x0 + x, y, z0 + z);
            before[((y - minY) * 16 + z) * 16 + x] = getState.invoke(chunk, pos);
        }
        long[] biomeQueries = {0};
        java.util.function.Function<Object, Object> biomes = position -> {
            try {
                ++biomeQueries[0];
                int x = (int) call(MC + "core.Vec3i", "getX", position, new String[]{});
                int y = (int) call(MC + "core.Vec3i", "getY", position, new String[]{});
                int z = (int) call(MC + "core.Vec3i", "getZ", position, new String[]{});
                return call(SOURCE, "getNoiseBiome", state.source,
                    new String[]{"int", "int", "int", CLIMATE + "Sampler"},
                    Math.floorDiv(x, 4), Math.floorDiv(y, 4), Math.floorDiv(z, 4), state.sampler);
            } catch (Exception error) { throw new IllegalStateException(error); }
        };
        Object returned = call(carver, "carve", configured,
            new String[]{LEVELGEN + "carver.CarvingContext", CHUNK + "ChunkAccess", "java.util.function.Function",
                MC + "util.RandomSource", LEVELGEN + "Aquifer", MC + "world.level.ChunkPos", CHUNK + "CarvingMask"},
            context, chunk, biomes, random, call(LEVELGEN + "NoiseChunk", "aquifer", noiseChunk, new String[]{}), chunkPos, mask);
        int changed = 0, masked = 0;
        List<List<Integer>> maskRuns = new ArrayList<>();
        int runStart = -1;
        for (int y = minY; y < minY + height; ++y) for (int z = 0; z < 16; ++z) for (int x = 0; x < 16; ++x) {
            int index = ((y - minY) * 16 + z) * 16 + x;
            Object pos = construct(MC + "core.BlockPos", new String[]{"int", "int", "int"}, x0 + x, y, z0 + z);
            if (before[index] != getState.invoke(chunk, pos)) ++changed;
            boolean carved = (boolean) call(CHUNK + "CarvingMask", "get", mask, new String[]{"int", "int", "int"}, x, y, z);
            if (carved) { ++masked; if (runStart < 0) runStart = index; }
            else if (runStart >= 0) { maskRuns.add(List.of(runStart, index - runStart)); runStart = -1; }
        }
        if (runStart >= 0) maskRuns.add(List.of(runStart, height * 256 - runStart));
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("configured_carver", id);
        result.put("profile", "actual ConfiguredWorldCarver.carve directly on actual native SURFACE volume; explicit caller-selected configuration, not biome selection; target=start chunk; RandomSource.create(world seed), no isStartChunk gate, no neighboring start traversal; NOT applyCarvers or a CARVERS stage; End cross-preset probes are primitives, not End generation");
        result.put("returned", returned); result.put("changed_blocks", changed); result.put("masked_blocks", masked);
        result.put("mask_true_runs_yzx", maskRuns); result.put("biome_callback_queries", biomeQueries[0]);
        result.put("rng_next_long_after_carve", call(MC + "util.RandomSource", "nextLong", random, new String[]{}));
        result.put("registry_access", "actual tagged runtime RegistryDataLoader output, no proxy");
        return result;
    }

    private Object noise(State state, long seed, String dimension, List<int[]> chunks, boolean includeSurface,
                         String configuredCarverId) throws Exception {
        int minY = dimension.equals("overworld") ? -64 : 0;
        int height = dimension.equals("overworld") ? 384 : 256;
        Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null,
            new String[]{"int", "int"}, minY, height);
        Object factory = paletteFactory(state);
        Object generator = construct(GENERATOR, new String[]{SOURCE, MC + "core.Holder"}, state.source, state.settingsHolder);
        Object blender = call(BLENDER, "empty", null, new String[]{});
        Object fluidPicker = call(GENERATOR, "createFluidPicker", null, new String[]{SETTINGS}, state.settings);
        Object geometry = call(SETTINGS, "noiseSettings", state.settings, new String[]{});
        Map<String, Object> geometryDescription = new LinkedHashMap<>();
        for (String key : new String[]{"minY", "height", "getCellWidth", "getCellHeight"}) {
            geometryDescription.put(key, call(LEVELGEN + "NoiseSettings", key, geometry, new String[]{}));
        }
        LinkedHashMap<String, Integer> palette = new LinkedHashMap<>(), biomePalette = new LinkedHashMap<>();
        IdentityHashMap<Object, Integer> stateIds = new IdentityHashMap<>();
        List<Object> columns = new ArrayList<>();
        List<int[]> biomeOrder = new ArrayList<>();
        BiomeRegistry biomeRegistry = includeSurface ? loadedBiomeRegistry(state) : null;
        List<Object> surfaceContexts = new ArrayList<>();
        List<Object> carverContexts = new ArrayList<>();
        for (int[] position : chunks) {
            Object chunkPos = construct(MC + "world.level.ChunkPos", new String[]{"int", "int"}, position[0], position[1]);
            Object chunk = construct(CHUNK + "ProtoChunk",
                new String[]{MC + "world.level.ChunkPos", CHUNK + "UpgradeData", MC + "world.level.LevelHeightAccessor",
                    CHUNK + "PalettedContainerFactory", LEVELGEN + "blending.BlendingData"},
                chunkPos, field(CHUNK + "UpgradeData", "EMPTY"), bounds, factory, null);
            Object noiseChunk = call(LEVELGEN + "NoiseChunk", "forChunk", null,
                new String[]{CHUNK + "ChunkAccess", RANDOM_STATE, LEVELGEN + "DensityFunctions$BeardifierOrMarker",
                    SETTINGS, LEVELGEN + "Aquifer$FluidPicker", BLENDER},
                chunk, state.random, field(LEVELGEN + "Beardifier", "EMPTY"), state.settings, fluidPicker, blender);
            java.util.function.Function<Object, Object> cached = ignored -> noiseChunk;
            call(CHUNK + "ChunkAccess", "getOrCreateNoiseChunk", chunk,
                new String[]{"java.util.function.Function"}, cached);
            // Only the unused StructureManager context is absent. Cached native NoiseChunk
            // carries the genuine empty Beardifier and fluid picker for this explicit profile.
            Object resolver = Proxy.newProxyInstance(getClass().getClassLoader(),
                new Class<?>[]{type(MC + "world.level.biome.BiomeResolver")}, (proxy, method, args) -> {
                    if (!method.getName().equals("getNoiseBiome")) throw new UnsupportedOperationException(method.getName());
                    int x = (int) args[0], y = (int) args[1], z = (int) args[2];
                    if (biomeOrder.size() < 64) biomeOrder.add(new int[]{x, y, z});
                    return call(SOURCE, "getNoiseBiome", state.source,
                        new String[]{"int", "int", "int", CLIMATE + "Sampler"}, x, y, z, args[3]);
                });
            Object climate = call(LEVELGEN + "NoiseChunk", "cachedClimateSampler", noiseChunk,
                new String[]{ROUTER, "java.util.List"}, state.router,
                call(SETTINGS, "spawnTarget", state.settings, new String[]{}));
            call(CHUNK + "ChunkAccess", "fillBiomesFromNoise", chunk,
                new String[]{MC + "world.level.biome.BiomeResolver", CLIMATE + "Sampler"}, resolver, climate);
            call(CHUNK + "ProtoChunk", "setPersistedStatus", chunk,
                new String[]{CHUNK + "status.ChunkStatus"}, field(CHUNK + "status.ChunkStatus", "BIOMES"));
            var future = (java.util.concurrent.CompletableFuture<?>) call(GENERATOR, "fillFromNoise", generator,
                new String[]{BLENDER, RANDOM_STATE, MC + "world.level.StructureManager", CHUNK + "ChunkAccess"},
                blender, state.random, null, chunk);
            if (future.join() != chunk) throw new IllegalStateException("native fill returned another chunk");
            call(CHUNK + "ProtoChunk", "setPersistedStatus", chunk,
                new String[]{CHUNK + "status.ChunkStatus"}, field(CHUNK + "status.ChunkStatus", "NOISE"));
            if (includeSurface) surfaceContexts.add(surface(state, seed, position, chunk, generator, blender,
                bounds, factory, fluidPicker, biomeRegistry));
            if (configuredCarverId != null) carverContexts.add(configuredCarver(state, seed, configuredCarverId,
                generator, bounds, chunk, noiseChunk, chunkPos, minY, height));
            List<Integer> blocks = new ArrayList<>();
            int last = -1, count = 0;
            Method getState = method(CHUNK + "ProtoChunk", "getBlockState", MC + "core.BlockPos");
            for (int y = minY; y < minY + height; ++y) {
                for (int z = 0; z < 16; ++z) for (int x = 0; x < 16; ++x) {
                    Object pos = construct(MC + "core.BlockPos", new String[]{"int", "int", "int"},
                        position[0] * 16 + x, y, position[1] * 16 + z);
                    Object block = getState.invoke(chunk, pos);
                    Integer id = stateIds.get(block);
                    if (id == null) {
                        String canonical = canonicalState(block);
                        id = palette.computeIfAbsent(canonical, ignored -> palette.size());
                        stateIds.put(block, id);
                    }
                    if (id == last) ++count;
                    else {
                        if (count != 0) { blocks.add(last); blocks.add(count); }
                        last = id; count = 1;
                    }
                }
            }
            blocks.add(last); blocks.add(count);
            int[] biomes = new int[height * 4];
            int index = 0;
            for (int y = minY / 4; y < (minY + height) / 4; ++y) {
                for (int z = 0; z < 4; ++z) for (int x = 0; x < 4; ++x) {
                    Object holder = call(CHUNK + "ChunkAccess", "getNoiseBiome", chunk,
                        new String[]{"int", "int", "int"}, position[0] * 4 + x, y, position[1] * 4 + z);
                    String name = (String) call(MC + "core.Holder", "getRegisteredName", holder, new String[]{});
                    biomes[index++] = biomePalette.computeIfAbsent(name, ignored -> biomePalette.size());
                }
            }
            Map<String, int[]> heightmaps = new TreeMap<>();
            Collection<?> nativeMaps = (Collection<?>) call(CHUNK + "ChunkAccess", "getHeightmaps", chunk, new String[]{});
            for (Object value : nativeMaps) {
                Map.Entry<?, ?> entry = (Map.Entry<?, ?>) value;
                String key = (String) call(LEVELGEN + "Heightmap$Types", "getSerializationKey", entry.getKey(), new String[]{});
                int[] heights = new int[256];
                for (int z = 0; z < 16; ++z) for (int x = 0; x < 16; ++x) {
                    heights[z * 16 + x] = (int) call(LEVELGEN + "Heightmap", "getFirstAvailable",
                        entry.getValue(), new String[]{"int", "int"}, x, z);
                }
                heightmaps.put(key, heights);
            }
            for (String getter : new String[]{"getBlockEntities", "getBlockEntityNbts"}) {
                if (!((Map<?, ?>) call(CHUNK + "ProtoChunk", getter, chunk, new String[]{})).isEmpty()) {
                    throw new IllegalStateException("unexpected block actor at " + (includeSurface ? "SURFACE" : "NOISE") + ": " + getter);
                }
            }
            if (!((List<?>) call(CHUNK + "ProtoChunk", "getEntities", chunk, new String[]{})).isEmpty()) {
                throw new IllegalStateException("unexpected entity at " + (includeSurface ? "SURFACE" : "NOISE"));
            }
            columns.add(Map.of("x", position[0], "z", position[1], "status", includeSurface ? "minecraft:surface" : "minecraft:noise",
                "blocks", blocks, "biomes", biomes, "heightmaps", heightmaps, "block_entities", List.of(),
                "snapshot_kind", configuredCarverId == null ? "native-generation-stage" : "configured-carver-primitive",
                "block_tick_count", call(MC + "world.ticks.TickAccess", "count",
                    call(CHUNK + "ChunkAccess", "getBlockTicks", chunk, new String[]{}), new String[]{}),
                "fluid_tick_count", call(MC + "world.ticks.TickAccess", "count",
                    call(CHUNK + "ChunkAccess", "getFluidTicks", chunk, new String[]{}), new String[]{})));
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("edition", "java");
        result.put("state_encoding", "java-canonical-properties-v1");
        result.put("dimension", dimensionId(dimension));
        result.put("min_y", minY); result.put("height", height);
        result.put("capture", includeSurface
            ? "native standalone NoiseBasedChunkGenerator.fillFromNoise then buildSurface; unblended; structures=false; actual nine-chunk native biome neighborhood; no carvers/features/ticks; declared preset inputs, not level.dat"
            : "native standalone NoiseBasedChunkGenerator.fillFromNoise; unblended; structures=false; no surface/carvers/features/ticks; declared preset inputs, not level.dat");
        result.put("structures", false); result.put("datapacks", List.of("vanilla"));
        result.put("inputs", declaredInputs(seed, dimension));
        result.put("palette", palette.keySet()); result.put("biome_palette", biomePalette.keySet());
        result.put("chunks", columns);
        result.put("noise_geometry", geometryDescription);
        result.put("first_section_query_order", biomeOrder);
        result.put("query_history", "shared source across chunks in argument order; native ChunkAccess.fillBiomesFromNoise traversal; cached native Climate.Sampler");
        result.put("actor_stage_truth", "native block actor maps and entity list observed empty at " + (includeSurface ? "SURFACE" : "NOISE"));
        if (includeSurface) {
            result.put("surface_contexts", surfaceContexts);
            result.put("surface_biome_registry", Map.of("native_type", MC + "core.MappedRegistry",
                "native_loaded_entries", biomeRegistry.entries, "native_loaded_tags", biomeRegistry.tags,
                "identity", "native RegistryDataLoader.WORLDGEN_REGISTRIES from actual vanilla PackResources; actual builtin pending tags applied; frozen native registry, no proxy"));
            result.put("native_settings_use_legacy_random_source", call(SETTINGS, "useLegacyRandomSource", state.settings, new String[]{}));
        }
        if (configuredCarverId != null) {
            result.put("capture", "configured-carver primitive after native SURFACE; NOT CARVERS/FEATURES stage; chunk persisted status remains SURFACE; no ticks or entity spawning executed");
            result.put("configured_carver_contexts", carverContexts);
            result.put("actor_stage_truth", "native block actor maps and entity list observed empty after configured-carver primitive; tick counts captured separately, ticks not executed");
        }
        return result;
    }

    private Object endBoundary(State state, List<int[]> points) throws Exception {
        List<Object> rows = new ArrayList<>();
        String[] densityTypes = new String[7];
        Arrays.fill(densityTypes, 0, 6, DENSITY);
        densityTypes[6] = "java.util.List";
        // Synthetic native constant densities are primitive selector probes, not
        // ordinary seed-generated biome references.
        for (int step = -256; step <= 256; ++step) {
            double center = step / 256.0;
            for (double value : new double[]{Math.nextDown(center), center, Math.nextUp(center)}) {
                Object constant = call(LEVELGEN + "DensityFunctions", "constant", null, new String[]{"double"}, value);
                Object sampler = construct(CLIMATE + "Sampler", densityTypes,
                    constant, constant, constant, constant, constant, constant, List.of());
                for (int[] point : points) {
                    rows.add(Map.of("quart", point, "constant_density_f64_bits",
                        String.format(Locale.ROOT, "%016x", Double.doubleToRawLongBits(value)),
                        "biome", biome(state.source, sampler, point[0], point[1], point[2])));
                }
            }
        }
        return Map.of("profile", "primitive_probe_native_constant_densities_not_vanilla_reference", "points", rows);
    }

    private void capturedDensities(Object object, String path, Map<String, Object> found) throws Exception {
        if (object == null) return;
        if (type(DENSITY).isInstance(object)) {
            found.put(path, object);
        } else if (object.getClass().isArray()) {
            for (int i = 0; i < Array.getLength(object); ++i) capturedDensities(Array.get(object, i), path + "[" + i + "]", found);
        } else if (type(LEVELGEN + "NoiseChunk$BlockStateFiller").isInstance(object)) {
            for (var field : object.getClass().getDeclaredFields()) {
                if (java.lang.reflect.Modifier.isStatic(field.getModifiers())) continue;
                field.setAccessible(true);
                Object value = field.get(object);
                if (value != object) capturedDensities(value, path + "." + field.getName(), found);
            }
        }
    }

    private String officialClass(Class<?> type) {
        for (var entry : classes.entrySet()) if (entry.getValue().equals(type.getName())) return entry.getKey();
        return type.getName();
    }

    private Object densityTree(Object density, Object context, int remaining) throws Exception {
        String owner = officialClass(density.getClass());
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("native_type", owner);
        result.put("f64_bits_at_current_native_context", densityBits(density, context));
        Map<String, Object> fields = new LinkedHashMap<>();
        for (var field : density.getClass().getDeclaredFields()) {
            if (java.lang.reflect.Modifier.isStatic(field.getModifiers()) || field.isSynthetic()) continue;
            field.setAccessible(true);
            Object value = field.get(density);
            String name = field.getName();
            for (var entry : members.getOrDefault(owner, Map.of()).entrySet()) {
                if (!entry.getKey().contains("(") && entry.getValue().equals(name)) { name = entry.getKey(); break; }
            }
            if (value != null && type(DENSITY).isInstance(value) && value != density) {
                if (remaining > 0 && !owner.equals(LEVELGEN + "NoiseChunk$NoiseInterpolator")) {
                    fields.put(name, densityTree(value, context, remaining - 1));
                } else fields.put(name, Map.of("native_type", officialClass(value.getClass()), "not_recursed", true));
            } else if (value instanceof Double number) {
                fields.put(name, Map.of("f64_bits", String.format(Locale.ROOT, "%016x", Double.doubleToRawLongBits(number))));
            } else if (value instanceof Number || value instanceof Boolean) fields.put(name, value);
            else if (value instanceof Enum<?>) fields.put(name, value.toString());
        }
        result.put("fields", fields);
        return result;
    }

    private Object featureOrder(State state) throws Exception {
        Object generator = construct(GENERATOR, new String[]{SOURCE, MC + "core.Holder"}, state.source, state.settingsHolder);
        Object placedRegistry = registry(state.lookup, "PLACED_FEATURE");
        Map<Object, String> names = new IdentityHashMap<>();
        Stream<?> entries = (Stream<?>) call(MC + "core.HolderLookup", "listElements", placedRegistry, new String[]{});
        for (Object holder : entries.toList()) {
            names.put(call(MC + "core.Holder", "value", holder, new String[]{}),
                (String) call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}));
        }
        var supplier = (java.util.function.Supplier<?>) instanceField(CHUNK + "ChunkGenerator", "featuresPerStep", generator);
        List<?> nativeSteps = (List<?>) supplier.get();
        Object[] decorations = type(LEVELGEN + "GenerationStep$Decoration").getEnumConstants();
        List<Object> steps = new ArrayList<>();
        for (int step = 0; step < nativeSteps.size(); ++step) {
            Object data = nativeSteps.get(step);
            List<?> features = (List<?>) call(MC + "world.level.biome.FeatureSorter$StepFeatureData", "features", data, new String[]{});
            @SuppressWarnings("unchecked")
            var mapping = (java.util.function.ToIntFunction<Object>) call(MC + "world.level.biome.FeatureSorter$StepFeatureData",
                "indexMapping", data, new String[]{});
            List<Object> ordered = new ArrayList<>();
            for (int index = 0; index < features.size(); ++index) {
                Object feature = features.get(index);
                ordered.add(Map.of("list_index", index, "native_index_mapping", mapping.applyAsInt(feature),
                    "placed_feature", Objects.requireNonNull(names.get(feature), "unregistered native placed feature")));
            }
            steps.add(Map.of("step", step, "decoration", ((Enum<?>) decorations[step]).name(), "features", ordered));
        }
        List<Object> biomes = new ArrayList<>();
        Set<?> possible = (Set<?>) call(SOURCE, "possibleBiomes", state.source, new String[]{});
        for (Object holder : possible) {
            Object settings = call(CHUNK + "ChunkGenerator", "getBiomeGenerationSettings", generator, new String[]{MC + "core.Holder"}, holder);
            List<?> biomeSteps = (List<?>) call(MC + "world.level.biome.BiomeGenerationSettings", "features", settings, new String[]{});
            List<Object> rows = new ArrayList<>();
            for (int step = 0; step < biomeSteps.size(); ++step) {
                List<Object> features = new ArrayList<>();
                Object data = nativeSteps.get(step);
                @SuppressWarnings("unchecked")
                var mapping = (java.util.function.ToIntFunction<Object>) call(MC + "world.level.biome.FeatureSorter$StepFeatureData",
                    "indexMapping", data, new String[]{});
                for (Object featureHolder : (Iterable<?>) biomeSteps.get(step)) {
                    Object feature = call(MC + "core.Holder", "value", featureHolder, new String[]{});
                    features.add(Map.of("placed_feature", Objects.requireNonNull(names.get(feature)),
                        "native_global_step_index", mapping.applyAsInt(feature)));
                }
                rows.add(Map.of("step", step, "features_in_biome_order", features));
            }
            biomes.add(Map.of("biome", call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}), "steps", rows));
        }
        return Map.of("profile", "actual native NoiseBasedChunkGenerator preset constructor; actual ChunkGenerator.featuresPerStep memoized supplier; actual native FeatureSorter.StepFeatureData.features/indexMapping; tagged vanilla runtime registries; no world, server, placement or manually reconstructed sort",
            "supplier_returns_same_memoized_list", supplier.get() == nativeSteps, "global_steps", steps,
            "possible_biomes_in_native_source_iteration_order", biomes);
    }

    private Map<String, Object> fluidStatus(Object status, int y) throws Exception {
        return Map.of("fluid_level", instanceField(LEVELGEN + "Aquifer$FluidStatus", "fluidLevel", status),
            "fluid_type", canonicalState(instanceField(LEVELGEN + "Aquifer$FluidStatus", "fluidType", status)),
            "at_query_y", canonicalState(call(LEVELGEN + "Aquifer$FluidStatus", "at", status, new String[]{"int"}, y)));
    }

    private Object aquiferDiagnostics(Object aquifer, Object context, int[] point) throws Exception {
        String owner = LEVELGEN + "Aquifer$NoiseBasedAquifer";
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("profile", "native reflective diagnostics AFTER actual computeSubstance returned, at same actual NoiseChunk context; pressure calls below are diagnostic native invocations, not instrumentation of private calls");
        result.put("native_type", officialClass(aquifer.getClass()));
        if (!type(owner).isInstance(aquifer)) return result;
        int minX = (int) instanceField(owner, "minGridX", aquifer);
        int minZ = (int) instanceField(owner, "minGridZ", aquifer);
        int sizeX = (int) instanceField(owner, "gridSizeX", aquifer);
        int sizeZ = (int) instanceField(owner, "gridSizeZ", aquifer);
        int x0 = (int) call(owner, "fromGridX", null, new String[]{"int", "int"}, minX, 0);
        int z0 = (int) call(owner, "fromGridZ", null, new String[]{"int", "int"}, minZ, 0);
        int x1 = (int) call(owner, "fromGridX", null, new String[]{"int", "int"}, minX + sizeX - 1, 9);
        int z1 = (int) call(owner, "fromGridZ", null, new String[]{"int", "int"}, minZ + sizeZ - 1, 9);
        result.put("max_surface_sample_bounds", new int[]{x0, z0, x1, z1});
        result.put("max_preliminary_surface_level", call(LEVELGEN + "NoiseChunk", "maxPreliminarySurfaceLevel",
            instanceField(owner, "noiseChunk", aquifer), new String[]{"int", "int", "int", "int"}, x0, z0, x1, z1));
        result.put("skip_sampling_above_y", instanceField(owner, "skipSamplingAboveY", aquifer));
        result.put("barrier_noise_f64_bits", densityBits(instanceField(owner, "barrierNoise", aquifer), context));
        Object global = call(LEVELGEN + "Aquifer$FluidPicker", "computeFluid", instanceField(owner, "globalFluidPicker", aquifer),
            new String[]{"int", "int", "int"}, point[0], point[1], point[2]);
        result.put("global_fluid_status", fluidStatus(global, point[1]));
        int gx = (int) call(owner, "gridX", null, new String[]{"int"}, point[0] - 5);
        int gy = (int) call(owner, "gridY", null, new String[]{"int"}, point[1] + 1);
        int gz = (int) call(owner, "gridZ", null, new String[]{"int"}, point[2] - 5);
        long[] locations = (long[]) instanceField(owner, "aquiferLocationCache", aquifer);
        List<int[]> centers = new ArrayList<>();
        List<Object> candidateRows = new ArrayList<>();
        int ordinal = 0;
        for (int dx = 0; dx <= 1; ++dx) for (int dy = -1; dy <= 1; ++dy) for (int dz = 0; dz <= 1; ++dz) {
            int index = (int) call(owner, "getIndex", aquifer, new String[]{"int", "int", "int"}, gx + dx, gy + dy, gz + dz);
            long packed = locations[index];
            if (packed == Long.MAX_VALUE) {
                candidateRows.add(Map.of("cache_index", index, "native_scan_ordinal", ordinal++, "location_cached", false));
                continue;
            }
            int x = (int) call(MC + "core.BlockPos", "getX", null, new String[]{"long"}, packed);
            int y = (int) call(MC + "core.BlockPos", "getY", null, new String[]{"long"}, packed);
            int z = (int) call(MC + "core.BlockPos", "getZ", null, new String[]{"long"}, packed);
            int distance = (x - point[0]) * (x - point[0]) + (y - point[1]) * (y - point[1]) + (z - point[2]) * (z - point[2]);
            centers.add(new int[]{distance, ordinal, index, x, y, z});
            candidateRows.add(Map.of("cache_index", index, "native_scan_ordinal", ordinal++, "location_cached", true,
                "center", new int[]{x, y, z}, "distance_squared", distance));
        }
        result.put("actual_cached_candidate_locations", candidateRows);
        // Native <= insertion makes the later native scan ordinal win a tie.
        centers.sort(Comparator.<int[]>comparingInt(row -> row[0]).thenComparing((a, b) -> Integer.compare(b[1], a[1])));
        List<Object> nearest = new ArrayList<>();
        List<Object> statuses = new ArrayList<>();
        Object[] cache = (Object[]) instanceField(owner, "aquiferCache", aquifer);
        for (int i = 0; i < Math.min(4, centers.size()); ++i) {
            int[] center = centers.get(i);
            boolean alreadyCached = cache[center[2]] != null;
            Object status = call(owner, "getAquiferStatus", aquifer, new String[]{"int"}, center[2]);
            statuses.add(status);
            nearest.add(Map.of("rank", i + 1, "center", new int[]{center[3], center[4], center[5]},
                "distance_squared", center[0], "cache_index", center[2], "status_cached_after_actual_call", alreadyCached,
                "native_fluid_status", fluidStatus(status, point[1])));
        }
        result.put("nearest_four", nearest);
        List<Object> pressures = new ArrayList<>();
        Object barrier = construct("org.apache.commons.lang3.mutable.MutableDouble", new String[]{"double"}, Double.NaN);
        for (int i = 0; i < statuses.size(); ++i) for (int j = i + 1; j < statuses.size(); ++j) {
            double before = ((Number) barrier.getClass().getMethod("doubleValue").invoke(barrier)).doubleValue();
            double pressure = (double) call(owner, "calculatePressure", aquifer,
                new String[]{DENSITY + "$FunctionContext", "org.apache.commons.lang3.mutable.MutableDouble",
                    LEVELGEN + "Aquifer$FluidStatus", LEVELGEN + "Aquifer$FluidStatus"}, context, barrier, statuses.get(i), statuses.get(j));
            double after = ((Number) barrier.getClass().getMethod("doubleValue").invoke(barrier)).doubleValue();
            double similarity = (double) call(owner, "similarity", null, new String[]{"int", "int"}, centers.get(i)[0], centers.get(j)[0]);
            pressures.add(Map.of("ranks", new int[]{i + 1, j + 1}, "first_status", fluidStatus(statuses.get(i), point[1]),
                "second_status", fluidStatus(statuses.get(j), point[1]), "query_y", point[1],
                "barrier_mutable_before_f64_bits", f64(before), "barrier_mutable_after_f64_bits", f64(after),
                "native_pressure_f64_bits", f64(pressure), "native_similarity_f64_bits", f64(similarity)));
        }
        result.put("diagnostic_native_pressure_pairs", pressures);
        return result;
    }

    private Object densityTrace(State state, List<int[]> points, boolean traceAquifer) throws Exception {
        Map<String, List<int[]>> grouped = new LinkedHashMap<>();
        for (int[] point : points) {
            String key = Math.floorDiv(point[0], 16) + ":" + Math.floorDiv(point[2], 16);
            grouped.computeIfAbsent(key, ignored -> new ArrayList<>()).add(point);
        }
        Object geometry = call(SETTINGS, "noiseSettings", state.settings, new String[]{});
        int minY = (int) call(LEVELGEN + "NoiseSettings", "minY", geometry, new String[]{});
        int height = (int) call(LEVELGEN + "NoiseSettings", "height", geometry, new String[]{});
        int width = (int) call(LEVELGEN + "NoiseSettings", "getCellWidth", geometry, new String[]{});
        int cellHeight = (int) call(LEVELGEN + "NoiseSettings", "getCellHeight", geometry, new String[]{});
        Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null,
            new String[]{"int", "int"}, minY, height);
        Object factory = paletteFactory(state);
        Object generator = construct(GENERATOR, new String[]{SOURCE, MC + "core.Holder"}, state.source, state.settingsHolder);
        Object blender = call(BLENDER, "empty", null, new String[]{});
        Object fluidPicker = call(GENERATOR, "createFluidPicker", null, new String[]{SETTINGS}, state.settings);
        List<Object> rows = new ArrayList<>();
        for (var group : grouped.entrySet()) {
            int[] chunkPosition = coordinates(group.getKey(), 2);
            Object chunkPos = construct(MC + "world.level.ChunkPos", new String[]{"int", "int"},
                chunkPosition[0], chunkPosition[1]);
            Object chunk = construct(CHUNK + "ProtoChunk",
                new String[]{MC + "world.level.ChunkPos", CHUNK + "UpgradeData", MC + "world.level.LevelHeightAccessor",
                    CHUNK + "PalettedContainerFactory", LEVELGEN + "blending.BlendingData"},
                chunkPos, field(CHUNK + "UpgradeData", "EMPTY"), bounds, factory, null);
            Object noiseChunk = call(LEVELGEN + "NoiseChunk", "forChunk", null,
                new String[]{CHUNK + "ChunkAccess", RANDOM_STATE, LEVELGEN + "DensityFunctions$BeardifierOrMarker",
                    SETTINGS, LEVELGEN + "Aquifer$FluidPicker", BLENDER},
                chunk, state.random, field(LEVELGEN + "Beardifier", "EMPTY"), state.settings, fluidPicker, blender);
            java.util.function.Function<Object, Object> cached = ignored -> noiseChunk;
            call(CHUNK + "ChunkAccess", "getOrCreateNoiseChunk", chunk,
                new String[]{"java.util.function.Function"}, cached);
            Object climate = call(LEVELGEN + "NoiseChunk", "cachedClimateSampler", noiseChunk,
                new String[]{ROUTER, "java.util.List"}, state.router,
                call(SETTINGS, "spawnTarget", state.settings, new String[]{}));
            call(CHUNK + "ChunkAccess", "fillBiomesFromNoise", chunk,
                new String[]{MC + "world.level.biome.BiomeResolver", CLIMATE + "Sampler"}, state.source, climate);
            call(CHUNK + "ProtoChunk", "setPersistedStatus", chunk,
                new String[]{CHUNK + "status.ChunkStatus"}, field(CHUNK + "status.ChunkStatus", "BIOMES"));
            Map<String, Object> captured = new LinkedHashMap<>();
            var ruleField = type(LEVELGEN + "NoiseChunk").getDeclaredField(members.get(LEVELGEN + "NoiseChunk").get("blockStateRule"));
            ruleField.setAccessible(true);
            Object nativeRule = ruleField.get(noiseChunk);
            capturedDensities(nativeRule, "native_block_state_rule", captured);
            if (captured.isEmpty()) throw new IllegalStateException("native material rule captured no observable density functions");
            Map<String, Map<String, Object>> observed = new LinkedHashMap<>();
            Method blockX = method(DENSITY + "$FunctionContext", "blockX");
            Method blockY = method(DENSITY + "$FunctionContext", "blockY");
            Method blockZ = method(DENSITY + "$FunctionContext", "blockZ");
            Method calculate = method(LEVELGEN + "NoiseChunk$BlockStateFiller", "calculate", DENSITY + "$FunctionContext");
            calculate.setAccessible(true);
            Map<String, Object> aquiferCalls = new LinkedHashMap<>();
            if (traceAquifer) {
                Object nativeAquifer = call(LEVELGEN + "NoiseChunk", "aquifer", noiseChunk, new String[]{});
                Method compute = method(LEVELGEN + "Aquifer", "computeSubstance", DENSITY + "$FunctionContext", "double");
                Method schedule = method(LEVELGEN + "Aquifer", "shouldScheduleFluidUpdate");
                Object aquiferObserver = Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{type(LEVELGEN + "Aquifer")},
                    (proxy, invoked, arguments) -> {
                        if (invoked.getName().equals(schedule.getName()) && invoked.getParameterCount() == 0) return schedule.invoke(nativeAquifer);
                        Object context = arguments[0];
                        int[] point = {(int) blockX.invoke(context), (int) blockY.invoke(context), (int) blockZ.invoke(context)};
                        Object answer = compute.invoke(nativeAquifer, arguments);
                        for (int[] requested : group.getValue()) {
                            if (!Arrays.equals(requested, point)) continue;
                            Map<String, Object> capture = new LinkedHashMap<>();
                            capture.put("actual_consumed_density_f64_bits", f64((double) arguments[1]));
                            capture.put("actual_compute_substance_result", answer == null ? "<null: native generator uses defaultBlock>" : canonicalState(answer));
                            capture.put("actual_should_schedule_fluid_update", schedule.invoke(nativeAquifer));
                            capture.put("function_context_is_actual_noise_chunk", context == noiseChunk);
                            capture.put("native_post_call_diagnostics", aquiferDiagnostics(nativeAquifer, context, point));
                            aquiferCalls.put(Arrays.toString(point), capture);
                        }
                        return answer;
                    });
                var aquiferField = type(LEVELGEN + "NoiseChunk").getDeclaredField(members.get(LEVELGEN + "NoiseChunk").get("aquifer"));
                aquiferField.setAccessible(true);
                aquiferField.set(noiseChunk, aquiferObserver);
            }
            long[] calls = {0};
            Object observer = Proxy.newProxyInstance(getClass().getClassLoader(),
                new Class<?>[]{type(LEVELGEN + "NoiseChunk$BlockStateFiller")}, (proxy, method, args) -> {
                    if (!method.getName().equals("calculate")) throw new UnsupportedOperationException(method.getName());
                    ++calls[0];
                    Object context = args[0];
                    int x = (int) blockX.invoke(context);
                    int y = (int) blockY.invoke(context);
                    int z = (int) blockZ.invoke(context);
                    int[] point = null;
                    for (int[] requested : group.getValue()) {
                        if (requested[0] == x && requested[1] == y && requested[2] == z) { point = requested; break; }
                    }
                    Map<String, Object> observation = null;
                    if (point != null) {
                        observation = new LinkedHashMap<>();
                        observation.put("block", point);
                        observation.put("native_fill_query_index", calls[0] - 1);
                        observation.put("function_context_is_actual_noise_chunk", context == noiseChunk);
                        Map<String, String> nativeDensityBits = new LinkedHashMap<>();
                        for (var entry : captured.entrySet()) nativeDensityBits.put(entry.getKey(), densityBits(entry.getValue(), context));
                        observation.put("actual_native_rule_captured_density_f64_bits", nativeDensityBits);
                        Map<String, Object> trees = new LinkedHashMap<>();
                        for (var entry : captured.entrySet()) trees.put(entry.getKey(), densityTree(entry.getValue(), context, 10));
                        observation.put("actual_native_rule_captured_density_trees", trees);
                        observation.put("raw_router", routerPoint(state, point));
                        Map<String, Object> counters = new LinkedHashMap<>();
                        for (String name : new String[]{"cellStartBlockX", "cellStartBlockY", "cellStartBlockZ",
                                "inCellX", "inCellY", "inCellZ", "interpolationCounter",
                                "arrayInterpolationCounter", "interpolating", "fillingCell"}) {
                            var field = type(LEVELGEN + "NoiseChunk").getDeclaredField(members.get(LEVELGEN + "NoiseChunk").get(name));
                            field.setAccessible(true);
                            counters.put(name, field.get(noiseChunk));
                        }
                        observation.put("native_noise_chunk_context", counters);
                        List<Object> corners = new ArrayList<>();
                        int x0 = Math.floorDiv(x, width) * width;
                        int y0 = Math.floorDiv(y, cellHeight) * cellHeight;
                        int z0 = Math.floorDiv(z, width) * width;
                        for (int dx : new int[]{0, width}) for (int dy : new int[]{0, cellHeight}) {
                            for (int dz : new int[]{0, width}) corners.add(routerPoint(state, new int[]{x0 + dx, y0 + dy, z0 + dz}));
                        }
                        observation.put("raw_router_eight_cell_corners", corners);
                    }
                    Object result = calculate.invoke(nativeRule, args);
                    if (observation != null) {
                        observation.put("native_get_interpolated_state", result == null ? "<null: native generator uses defaultBlock>" : canonicalState(result));
                        if (traceAquifer) observation.put("actual_aquifer_call", Objects.requireNonNull(aquiferCalls.get(Arrays.toString(point))));
                        observed.put(Arrays.toString(point), observation);
                    }
                    return result;
                });
            ruleField.set(noiseChunk, observer);
            var future = (java.util.concurrent.CompletableFuture<?>) call(GENERATOR, "fillFromNoise", generator,
                new String[]{BLENDER, RANDOM_STATE, MC + "world.level.StructureManager", CHUNK + "ChunkAccess"},
                blender, state.random, null, chunk);
            if (future.join() != chunk) throw new IllegalStateException("native fill returned another chunk");
            for (int[] point : group.getValue()) {
                Map<String, Object> observation = observed.get(Arrays.toString(point));
                if (observation == null) throw new IllegalStateException("native fill did not visit requested point " + Arrays.toString(point));
                Object position = construct(MC + "core.BlockPos", new String[]{"int", "int", "int"},
                    point[0], point[1], point[2]);
                Object block = call(CHUNK + "ProtoChunk", "getBlockState", chunk, new String[]{MC + "core.BlockPos"}, position);
                observation.put("actual_full_volume_state_after_native_fill", canonicalState(block));
                observation.put("native_default_block", canonicalState(call(SETTINGS, "defaultBlock", state.settings, new String[]{})));
                observation.put("native_fill_query_count", calls[0]);
                rows.add(observation);
            }
        }
        return Map.of("profile", "native real fillFromNoise traversal; unblended; empty Beardifier; structures=false; passive delegating BlockStateFiller observer; no manual interpolation traversal",
            "cell_width", width, "cell_height", cellHeight, "noise_min_y", minY, "noise_height", height, "points", rows);
    }

    private Object instanceField(Object receiver, String name) throws Exception {
        String owner = officialClass(receiver.getClass());
        var field = receiver.getClass().getDeclaredField(Objects.requireNonNull(members.get(owner).get(name), owner + "." + name));
        field.setAccessible(true);
        return field.get(receiver);
    }

    private static String f64(double value) {
        return String.format(Locale.ROOT, "%016x", Double.doubleToRawLongBits(value));
    }

    private static String f32(float value) {
        return String.format(Locale.ROOT, "%08x", Float.floatToRawIntBits(value));
    }

    private Object numericTree(Object object, Object context, Object splinePoint, int remaining) throws Exception {
        String owner = officialClass(object.getClass());
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("native_type", owner);
        boolean density = type(DENSITY).isInstance(object);
        boolean spline = type(MC + "util.CubicSpline").isInstance(object);
        if (density) result.put("f64_bits", densityBits(object, context));
        if (spline) {
            float value = (float) call(MC + "util.BoundedFloatFunction", "apply", object,
                new String[]{"java.lang.Object"}, splinePoint);
            result.put("spline_output_f32_bits", f32(value));
            result.put("spline_output_widened_f64_bits", f64((double) value));
        }
        if (remaining == 0) { result.put("not_recursed", true); return result; }
        if (owner.equals(LEVELGEN + "DensityFunctions$ShiftedNoise")) {
            double xz = (double) instanceField(object, "xzScale"), ys = (double) instanceField(object, "yScale");
            double sx = (double) call(DENSITY, "compute", instanceField(object, "shiftX"), new String[]{DENSITY + "$FunctionContext"}, context);
            double sy = (double) call(DENSITY, "compute", instanceField(object, "shiftY"), new String[]{DENSITY + "$FunctionContext"}, context);
            double sz = (double) call(DENSITY, "compute", instanceField(object, "shiftZ"), new String[]{DENSITY + "$FunctionContext"}, context);
            int x = (int) call(DENSITY + "$FunctionContext", "blockX", context, new String[]{});
            int y = (int) call(DENSITY + "$FunctionContext", "blockY", context, new String[]{});
            int z = (int) call(DENSITY + "$FunctionContext", "blockZ", context, new String[]{});
            double nx = x * xz + sx, ny = y * ys + sy, nz = z * xz + sz;
            Object holder = instanceField(object, "noise");
            Object noise = call(DENSITY + "$NoiseHolder", "noise", holder, new String[]{});
            Object data = call(DENSITY + "$NoiseHolder", "noiseData", holder, new String[]{});
            String name = (String) call(MC + "core.Holder", "getRegisteredName", data, new String[]{});
            double direct = (double) call(LEVELGEN + "synth.NormalNoise", "getValue", noise,
                new String[]{"double", "double", "double"}, nx, ny, nz);
            double held = (double) call(DENSITY + "$NoiseHolder", "getValue", holder,
                new String[]{"double", "double", "double"}, nx, ny, nz);
            double unshifted = (double) call(LEVELGEN + "synth.NormalNoise", "getValue", noise,
                new String[]{"double", "double", "double"}, x * xz, y * ys, z * xz);
            result.put("direct_named_normal_noise_probe", Map.of("noise_name", name,
                "input_construction", "block coordinate times actual native scale plus actual native shift compute; declared probe arguments",
                "shift_f64_bits", List.of(f64(sx), f64(sy), f64(sz)),
                "get_value_arguments_f64_bits", List.of(f64(nx), f64(ny), f64(nz)),
                "normal_noise_get_value_f64_bits", f64(direct), "noise_holder_get_value_f64_bits", f64(held),
                "unshifted_scaled_input_f64_bits", List.of(f64(x * xz), f64(y * ys), f64(z * xz)),
                "unshifted_normal_noise_get_value_f64_bits", f64(unshifted)));
        }
        if (owner.equals(MC + "util.CubicSpline$Multipoint")) {
            Object coordinate = instanceField(object, "coordinate");
            float value = (float) call(MC + "util.BoundedFloatFunction", "apply", coordinate,
                new String[]{"java.lang.Object"}, splinePoint);
            result.put("coordinate_native_type", officialClass(coordinate.getClass()));
            result.put("coordinate_output_f32_bits", f32(value));
            if (officialClass(coordinate.getClass()).equals(LEVELGEN + "DensityFunctions$Spline$Coordinate")) {
                Object holder = instanceField(coordinate, "function");
                Object function = call(MC + "core.Holder", "value", holder, new String[]{});
                result.put("coordinate_density_name", call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}));
                result.put("coordinate_density_f64_bits_before_native_float_conversion", densityBits(function, context));
            }
            float[] locations = (float[]) instanceField(object, "locations");
            result.put("native_interval_start", call(MC + "util.CubicSpline$Multipoint", "findIntervalStart", null,
                new String[]{"float[]", "float"}, locations, value));
        }
        Map<String, Object> fields = new LinkedHashMap<>();
        for (var field : object.getClass().getDeclaredFields()) {
            if (java.lang.reflect.Modifier.isStatic(field.getModifiers()) || field.isSynthetic()) continue;
            field.setAccessible(true);
            Object value = field.get(object);
            if (value == null || value == object) continue;
            String name = field.getName();
            for (var entry : members.getOrDefault(owner, Map.of()).entrySet()) {
                if (!entry.getKey().contains("(") && entry.getValue().equals(name)) { name = entry.getKey(); break; }
            }
            if (type(DENSITY).isInstance(value) || type(MC + "util.CubicSpline").isInstance(value)) {
                fields.put(name, numericTree(value, context, splinePoint, remaining - 1));
            } else if (type(MC + "core.Holder").isInstance(value)) {
                Object held = call(MC + "core.Holder", "value", value, new String[]{});
                fields.put(name, Map.of("registered_name", call(MC + "core.Holder", "getRegisteredName", value, new String[]{}),
                    "value", type(DENSITY).isInstance(held) ? numericTree(held, context, splinePoint, remaining - 1) : Map.of("native_type", officialClass(held.getClass()))));
            } else if (value instanceof Double number) fields.put(name, Map.of("f64_bits", f64(number)));
            else if (value instanceof Float number) fields.put(name, Map.of("f32_bits", f32(number), "widened_f64_bits", f64((double) number)));
            else if (value instanceof float[] numbers) {
                List<String> bits = new ArrayList<>();
                for (float number : numbers) bits.add(f32(number));
                fields.put(name, Map.of("f32_bits", bits));
            } else if (value instanceof List<?> list && spline) {
                List<Object> children = new ArrayList<>();
                for (Object child : list) children.add(numericTree(child, context, splinePoint, remaining - 1));
                fields.put(name, children);
            } else if (value instanceof Number || value instanceof Boolean) fields.put(name, value);
            else if (value instanceof Enum<?>) fields.put(name, value.toString());
        }
        result.put("fields", fields);
        return result;
    }

    private Object numericTrace(State state, List<int[]> points) throws Exception {
        List<Object> rows = new ArrayList<>();
        for (int[] point : points) {
            Object context = construct(DENSITY + "$SinglePointContext", new String[]{"int", "int", "int"}, point[0], point[1], point[2]);
            Object splinePoint = construct(LEVELGEN + "DensityFunctions$Spline$Point", new String[]{DENSITY + "$FunctionContext"}, context);
            Map<String, Object> roots = new LinkedHashMap<>();
            for (String name : new String[]{"continents", "erosion", "ridges", "depth"}) {
                roots.put(name, numericTree(call(ROUTER, name, state.router, new String[]{}), context, splinePoint, 32));
            }
            rows.add(Map.of("block", point, "raw_router", routerPoint(state, point), "native_numeric_roots", roots));
        }
        return Map.of("profile", "native raw RandomState router; SinglePointContext; native named NormalNoise calls; native CubicSpline apply and coordinate float observations; no spline implementation or interpolation inversion",
            "points", rows);
    }

    private static int[] coordinates(String text, int count) {
        int[] values = Arrays.stream(text.split(":")).mapToInt(Integer::parseInt).toArray();
        if (values.length != count) throw new IllegalArgumentException("wrong coordinate arity: " + text);
        return values;
    }
    private Object structureOrder(State state, long seed) throws Exception {
        List<Object> structures = new ArrayList<>();
        Stream<?> entries = (Stream<?>) call(MC + "core.HolderLookup", "listElements",
            registry(state.lookup, "STRUCTURE"), new String[]{});
        for (Object holder : entries.toList()) {
            Object structure = call(MC + "core.Holder", "value", holder, new String[]{});
            Enum<?> step = (Enum<?>) call(LEVELGEN + "structure.Structure", "step", structure, new String[]{});
            structures.add(Map.of("id", call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}),
                "step", step.ordinal()));
        }
        Object manager = call(CHUNK + "ChunkGeneratorStructureState", "createForNormal", null,
            new String[]{RANDOM_STATE, "long", SOURCE, MC + "core.HolderLookup"},
            state.random, seed, state.source, registry(state.lookup, "STRUCTURE_SET"));
        List<?> possible = (List<?>) call(CHUNK + "ChunkGeneratorStructureState", "possibleStructureSets",
            manager, new String[]{});
        List<String> sets = new ArrayList<>();
        for (Object holder : possible) {
            sets.add((String) call(MC + "core.Holder", "getRegisteredName", holder, new String[]{}));
        }
        return Map.of("profile", "actual runtime structure registry iteration and actual createForNormal possibleStructureSets; no starts, placement or full-feature capture",
            "structures", structures, "possible_structure_sets", sets);
    }

    private Object instanceField(String owner, String name, Object instance) throws Exception {
        var field = type(owner).getDeclaredField(Objects.requireNonNull(members.get(owner).get(name), owner + "." + name));
        field.setAccessible(true);
        return field.get(instance);
    }

    private Object columnConstructorFields(State state, String dimension, List<int[]> positions) throws Exception {
        Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null, new String[]{"int", "int"},
            dimension.equals("overworld") ? -64 : 0, dimension.equals("overworld") ? 384 : 256);
        Object geometry = call(LEVELGEN + "NoiseSettings", "clampToHeightAccessor",
            call(SETTINGS, "noiseSettings", state.settings, new String[]{}),
            new String[]{MC + "world.level.LevelHeightAccessor"}, bounds);
        int width = (int) call(LEVELGEN + "NoiseSettings", "getCellWidth", geometry, new String[]{});
        List<Object> rows = new ArrayList<>();
        for (int[] position : positions) {
            int originX = Math.floorDiv(position[0], width) * width;
            int originZ = Math.floorDiv(position[1], width) * width;
            Object noise = construct(LEVELGEN + "NoiseChunk",
                new String[]{"int", RANDOM_STATE, "int", "int", LEVELGEN + "NoiseSettings",
                    LEVELGEN + "DensityFunctions$BeardifierOrMarker", SETTINGS, LEVELGEN + "Aquifer$FluidPicker", BLENDER},
                1, state.random, originX, originZ, geometry, field(LEVELGEN + "DensityFunctions$BeardifierMarker", "INSTANCE"),
                state.settings, call(GENERATOR, "createFluidPicker", null, new String[]{SETTINGS}, state.settings),
                call(BLENDER, "empty", null, new String[]{}));
            Map<String, Object> fields = new LinkedHashMap<>();
            for (String name : new String[]{"firstCellX", "firstCellZ", "firstNoiseX", "firstNoiseZ",
                    "cellCountXZ", "cellCountY", "cellNoiseMinY", "noiseSizeXZ", "cellWidth", "cellHeight"}) {
                fields.put(name, instanceField(LEVELGEN + "NoiseChunk", name, noise));
            }
            Object aquifer = call(LEVELGEN + "NoiseChunk", "aquifer", noise, new String[]{});
            Map<String, Object> aq = new LinkedHashMap<>();
            aq.put("native_type", officialClass(aquifer.getClass()));
            if (type(LEVELGEN + "Aquifer$NoiseBasedAquifer").isInstance(aquifer)) {
                for (String name : new String[]{"minGridX", "minGridY", "minGridZ", "gridSizeX", "gridSizeZ", "skipSamplingAboveY"}) {
                    aq.put(name, instanceField(LEVELGEN + "Aquifer$NoiseBasedAquifer", name, aquifer));
                }
                for (String name : new String[]{"aquiferCache", "aquiferLocationCache"}) {
                    aq.put(name + "_length", Array.getLength(instanceField(LEVELGEN + "Aquifer$NoiseBasedAquifer", name, aquifer)));
                }
            }
            rows.add(Map.of("x", position[0], "z", position[1], "constructor_origin_x", originX,
                "constructor_origin_z", originZ, "noise_chunk", fields, "aquifer", aq));
        }
        return Map.of("profile", "directly constructed actual native NoiseChunk using pinned iterateNoiseColumn constructor arguments: cellCountXZ=1, floorDiv(block,cellWidth)*cellWidth origins, clamped native noise settings, native BeardifierMarker.INSTANCE, actual fluid picker, Blender.empty; fields read immediately after constructor; NOT instrumentation of public getBaseColumn",
            "clamped_min_y", call(LEVELGEN + "NoiseSettings", "minY", geometry, new String[]{}),
            "clamped_height", call(LEVELGEN + "NoiseSettings", "height", geometry, new String[]{}), "columns", rows);
    }

    private Object columns(State state, String dimension, List<int[]> positions) throws Exception {
        int minY = dimension.equals("overworld") ? -64 : 0;
        int height = dimension.equals("overworld") ? 384 : 256;
        Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null,
            new String[]{"int", "int"}, minY, height);
        Object generator = construct(GENERATOR, new String[]{SOURCE, MC + "core.Holder"},
            state.source, state.settingsHolder);
        Object geometry = call(SETTINGS, "noiseSettings", state.settings, new String[]{});
        Map<String, Object> settings = new LinkedHashMap<>();
        for (String key : new String[]{"minY", "height", "getCellWidth", "getCellHeight"}) {
            settings.put(key, call(LEVELGEN + "NoiseSettings", key, geometry, new String[]{}));
        }
        for (String key : new String[]{"seaLevel", "useLegacyRandomSource", "aquifersEnabled",
                "oreVeinsEnabled", "defaultBlock", "defaultFluid"}) {
            Object value = call(SETTINGS, key, state.settings, new String[]{});
            settings.put(key, key.startsWith("default") ? canonicalState(value) : value);
        }
        List<Object> rows = new ArrayList<>();
        Map<String, Object> randoms = new LinkedHashMap<>();
        Object regionFactory = call(RANDOM_STATE, "getOrCreateRandomFactory", state.random,
            new String[]{MC + "resources.Identifier"},
            call(MC + "resources.Identifier", "withDefaultNamespace", null,
                new String[]{"java.lang.String"}, "worldgen_region_random"));
        for (int[] position : positions) {
            int x = position[0], z = position[1];
            Object column = call(GENERATOR, "getBaseColumn", generator,
                new String[]{"int", "int", MC + "world.level.LevelHeightAccessor", RANDOM_STATE},
                x, z, bounds, state.random);
            List<String> blocks = new ArrayList<>(height);
            List<Object> nativeStates = new ArrayList<>(height);
            for (int y = minY; y < minY + height; ++y) {
                Object block = call(MC + "world.level.NoiseColumn", "getBlock", column,
                    new String[]{"int"}, y);
                blocks.add(canonicalState(block));
                nativeStates.add(block);
            }
            Map<String, Integer> heights = new LinkedHashMap<>(), scanned = new LinkedHashMap<>();
            for (String kind : new String[]{"WORLD_SURFACE_WG", "WORLD_SURFACE", "OCEAN_FLOOR_WG",
                    "OCEAN_FLOOR", "MOTION_BLOCKING", "MOTION_BLOCKING_NO_LEAVES"}) {
                Object type = field(LEVELGEN + "Heightmap$Types", kind);
                heights.put(kind, (int) call(GENERATOR, "getBaseHeight", generator,
                    new String[]{"int", "int", LEVELGEN + "Heightmap$Types",
                        MC + "world.level.LevelHeightAccessor", RANDOM_STATE}, x, z, type, bounds, state.random));
                @SuppressWarnings("unchecked")
                java.util.function.Predicate<Object> predicate = (java.util.function.Predicate<Object>)
                    call(LEVELGEN + "Heightmap$Types", "isOpaque", type, new String[]{});
                int top = minY;
                for (int y = height - 1; y >= 0; --y) {
                    if (predicate.test(nativeStates.get(y))) { top = minY + y + 1; break; }
                }
                scanned.put(kind, top);
            }
            if (!heights.equals(scanned)) {
                throw new IllegalStateException("native base-height/column mismatch at " + x + ":" + z);
            }
            rows.add(Map.of("x", x, "z", z, "states", blocks, "native_base_heights", heights,
                "native_column_predicate_heights", scanned));
            int cx = Math.floorDiv(x, 16), cz = Math.floorDiv(z, 16);
            String key = cx + ":" + cz;
            if (!randoms.containsKey(key)) {
                Object random = call(LEVELGEN + "PositionalRandomFactory", "at", regionFactory,
                    new String[]{"int", "int", "int"}, cx * 16, 0, cz * 16);
                List<Object> draws = new ArrayList<>();
                for (int i = 0; i < 16; ++i) {
                    draws.add(List.of(
                        call(MC + "util.RandomSource", "nextBoolean", random, new String[]{}),
                        call(MC + "util.RandomSource", "nextFloat", random, new String[]{}),
                        call(MC + "util.RandomSource", "nextInt", random, new String[]{"int"}, 3)));
                }
                randoms.put(key, Map.of("native_source", officialClass(random.getClass()),
                    "draw_order", "nextBoolean,nextFloat,nextInt(3); repeated 16 times", "draws", draws));
            }
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("capture", "actual native public NoiseBasedChunkGenerator.getBaseColumn/getBaseHeight; no chunk creation; no server; no structure placement");
        result.put("column_profile", "iterateNoiseColumn one horizontal cell; clamped noise settings; Beardifier.Marker.INSTANCE; Blender.empty; descending cellY/withinCellY, updateY/updateX/updateZ, native getInterpolatedState and native aquifer/material rule; null uses native defaultBlock");
        result.put("min_y", minY); result.put("height", height); result.put("noise_settings", settings);
        result.put("columns", rows); result.put("worldgen_region_random", randoms);
        return result;
    }


    public static void main(String[] args) {
        try {
            if (args.length < 5 || (args.length < 6 && !args[1].equals("feature-order"))) throw new IllegalArgumentException("<runtime-root> <point|router|biomes|noise|noise-runtime|surface|configured-carver|columns|column-constructor-fields|structure-order|feature-order|end-boundary|density-trace|aquifer-trace|numeric-trace|fullstage> <output.json> <seed> <overworld|nether|end|all-for-fullstage> <coordinates>...; feature-order takes no coordinates; configured-carver uses <configured-id> <chunk-x:chunk-z>...; fullstage prefix: [--structures=true|false] [--trace-native-tasks] [--native-village-anchors]");
            Path runtime = Path.of(args[0]);
            verify(runtime.resolve("server.jar"), SERVER_SHA1, "SHA-1");
            verify(runtime.resolve("server-mappings.txt"), MAPPINGS_SHA1, "SHA-1");
            verify(runtime.resolve("versions/1.21.11/server-1.21.11.jar"), INNER_SHA256, "SHA-256");
            if (args[1].equals("fullstage")) {
                if (!args[4].equals("all")) throw new IllegalArgumentException("fullstage captures all three dimensions; use dimension argument all");
                List<String> contextArgs = new ArrayList<>(List.of(args[0], args[2], args[3]));
                contextArgs.addAll(Arrays.asList(args).subList(5, args.length));
                Class<?> context = Class.forName("NativeContextProbe");
                context.getMethod("main", String[].class).invoke(null, (Object) contextArgs.toArray(String[]::new));
                return;
            }
            NativeWorldgenOracle oracle = new NativeWorldgenOracle(runtime.resolve("server-mappings.txt"));
            long seed = Long.parseLong(args[3]);
            boolean runtimeRegistries = args[1].equals("surface") || args[1].equals("noise-runtime")
                || args[1].equals("columns") || args[1].equals("structure-order") || args[1].equals("configured-carver")
                || args[1].equals("column-constructor-fields") || args[1].equals("aquifer-trace") || args[1].equals("feature-order");
            State state = oracle.bootstrap(seed, args[4], runtimeRegistries);
            Map<String, Object> output = new LinkedHashMap<>();
            if (runtimeRegistries) output.put("registry_profile", "native actual vanilla PackResources; builtin pending tags applied; native RegistryDataLoader WORLDGEN_REGISTRIES with native tags; not datagen lookup");
            if (args[1].equals("point") || args[1].equals("router")) {
                List<Object> rows = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) {
                    int[] position = coordinates(args[i], 3);
                    rows.add(args[1].equals("point") ? oracle.point(state, position) : oracle.routerPoint(state, position));
                }
                output.put("result", Map.of("points", rows));
            } else if (args[1].equals("biomes") || args[1].equals("noise") || args[1].equals("noise-runtime") || args[1].equals("surface")) {
                List<int[]> chunks = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) chunks.add(coordinates(args[i], 2));
                if (!args[1].equals("biomes")) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> capture = (Map<String, Object>) oracle.noise(state, seed, args[4], chunks, args[1].equals("surface"), null);
                    output.putAll(capture);
                } else output.put("result", oracle.biomes(state, args[4], chunks));
            } else if (args[1].equals("configured-carver")) {
                if (args.length < 7) throw new IllegalArgumentException("configured-carver requires <configured-id> <chunk-x:chunk-z>...");
                List<int[]> chunks = new ArrayList<>();
                for (int i = 6; i < args.length; ++i) chunks.add(coordinates(args[i], 2));
                @SuppressWarnings("unchecked")
                Map<String, Object> capture = (Map<String, Object>) oracle.noise(state, seed, args[4], chunks, true, args[5]);
                output.putAll(capture);
            } else if (args[1].equals("column-constructor-fields")) {
                List<int[]> positions = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) positions.add(coordinates(args[i], 2));
                output.put("result", oracle.columnConstructorFields(state, args[4], positions));
            } else if (args[1].equals("columns")) {
                List<int[]> positions = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) positions.add(coordinates(args[i], 2));
                output.put("result", oracle.columns(state, args[4], positions));
            } else if (args[1].equals("structure-order")) {
                output.put("result", oracle.structureOrder(state, seed));
            } else if (args[1].equals("feature-order")) {
                output.put("result", oracle.featureOrder(state));
            } else if (args[1].equals("density-trace") || args[1].equals("aquifer-trace")) {
                List<int[]> points = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) points.add(coordinates(args[i], 3));
                output.put("result", oracle.densityTrace(state, points, args[1].equals("aquifer-trace")));
            } else if (args[1].equals("numeric-trace")) {
                List<int[]> points = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) points.add(coordinates(args[i], 3));
                output.put("result", oracle.numericTrace(state, points));
            } else if (args[1].equals("end-boundary") && args[4].equals("end")) {
                List<int[]> points = new ArrayList<>();
                for (int i = 5; i < args.length; ++i) points.add(coordinates(args[i], 3));
                output.put("result", oracle.endBoundary(state, points));
            } else throw new IllegalArgumentException("unknown oracle operation/dimension " + args[1] + "/" + args[4]);
            output.put("version", "1.21.11"); output.put("server_sha1", SERVER_SHA1);
            output.put("mappings_sha1", MAPPINGS_SHA1); output.put("inner_sha256", INNER_SHA256);
            output.put("seed", seed); output.putIfAbsent("dimension", args[4]);
            output.put("operation", args[1]); output.put("possible_biomes", oracle.possibleBiomes(state));
            output.put("invocation", List.of(args));
            Files.writeString(Path.of(args[2]), new Gson().toJson(output));
            System.exit(0);
        } catch (Throwable error) {
            error.printStackTrace();
            System.exit(1);
        }
    }
}
