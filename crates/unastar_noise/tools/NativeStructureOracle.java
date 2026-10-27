/* Reference-only Java 1.21.11 structure assembly/query capture. No server or
 * level is constructed and no chunk stage is scheduled. A delegating generator
 * records only the real base-height/column queries used by native structures.
 * Usage: <runtime-root> <output.json> <seed> <overworld|nether|end>
 *        <structure-id@chunk-x:chunk-z> ...
 * Compile with the pinned inner server/libraries and NativeWorldgenOracle.
 */
import com.google.gson.Gson;
import com.mojang.serialization.MapCodec;
import java.lang.reflect.*;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.CompletableFuture;
import java.util.function.Predicate;

public final class NativeStructureOracle {
    static final String MC = "net.minecraft.", LG = MC + "world.level.levelgen.", ST = LG + "structure.";
    static Object oracle;
    static Object invoke(String name, Class<?>[] parameters, Object... arguments) throws Exception {
        Method method = oracle.getClass().getDeclaredMethod(name, parameters);
        method.setAccessible(true);
        try { return method.invoke(oracle, arguments); }
        catch (InvocationTargetException error) { throw new IllegalStateException(error.getCause()); }
    }
    static Object call(String owner, String name, Object receiver, String[] parameters, Object... arguments) throws Exception {
        return invoke("call", new Class[]{String.class, String.class, Object.class, String[].class, Object[].class}, owner, name, receiver, parameters, arguments);
    }
    static Object field(String owner, String name) throws Exception {
        return invoke("field", new Class[]{String.class, String.class}, owner, name);
    }
    static Object make(String owner, String[] parameters, Object... arguments) throws Exception {
        return invoke("construct", new Class[]{String.class, String[].class, Object[].class}, owner, parameters, arguments);
    }
    static Object component(Object record, String name) throws Exception {
        Method method = record.getClass().getDeclaredMethod(name);
        method.setAccessible(true);
        return method.invoke(record);
    }
    static String state(Object state) throws Exception {
        return (String) invoke("canonicalState", new Class[]{Object.class}, state);
    }
    static List<Integer> position(Object pos) throws Exception {
        List<Integer> result = new ArrayList<>();
        for (String name : List.of("getX", "getY", "getZ")) result.add((int) call(MC + "core.Vec3i", name, pos, new String[]{}));
        return result;
    }
    static List<Integer> box(Object box) throws Exception {
        List<Integer> result = new ArrayList<>();
        for (String name : List.of("minX", "minY", "minZ", "maxX", "maxY", "maxZ")) result.add((int) call(ST + "BoundingBox", name, box, new String[]{}));
        return result;
    }
    static final class Queries extends eqg {
        final eqg delegate;
        final List<Object> heights = new ArrayList<>(), columns = new ArrayList<>();
        Queries(eqg delegate) { super(delegate.d()); this.delegate = delegate; }
        protected MapCodec<? extends eqg> b() {
            try { return (MapCodec<? extends eqg>) call(MC + "world.level.chunk.ChunkGenerator", "codec", delegate, new String[]{}); }
            catch (Exception error) { throw new IllegalStateException(error); }
        }
        public void a(axn region, long seed, eve random, dxq biome, dxk structures, eqf chunk) { delegate.a(region, seed, random, biome, structures, chunk); }
        public void a(axn region, dxk structures, eve random, eqf chunk) { delegate.a(region, structures, random, chunk); }
        public void a(axn region) { delegate.a(region); }
        public CompletableFuture<eqf> a(evs blender, eve random, dxk structures, eqf chunk) { return delegate.a(blender, random, structures, chunk); }
        public int e() { return delegate.e(); }
        public int f() { return delegate.f(); }
        public int g() { return delegate.g(); }
        public int a(int x, int z, euq.a kind, dwq bounds, eve random) {
            int result = delegate.a(x, z, kind, bounds, random);
            heights.add(Map.of("x", x, "z", z, "kind", kind.toString(), "height", result));
            return result;
        }
        public dxa a(int x, int z, dwq bounds, eve random) {
            dxa result = delegate.a(x, z, bounds, random);
            try {
                List<String> blocks = new ArrayList<>();
                int min = (int) call(MC + "world.level.LevelHeightAccessor", "getMinY", bounds, new String[]{});
                int height = (int) call(MC + "world.level.LevelHeightAccessor", "getHeight", bounds, new String[]{});
                for (int y = min; y < min + height; ++y) blocks.add(state(call(MC + "world.level.NoiseColumn", "getBlock", result, new String[]{"int"}, y)));
                columns.add(Map.of("x", x, "z", z, "min_y", min, "blocks", blocks));
            } catch (Exception error) { throw new IllegalStateException(error); }
            return result;
        }
        public void a(List<String> lines, eve random, is pos) { delegate.a(lines, random, pos); }
    }
    static Object elementIdentity(Object element) throws Exception {
        if (element instanceof fgv) {
            return Map.of("kind", element instanceof fgt ? "legacy_single" : "single",
                "template", call(ST + "pools.SinglePoolElement", "getTemplateLocation", element, new String[]{}).toString());
        }
        if (element instanceof fgu) {
            List<Object> children = new ArrayList<>();
            for (Object child : (List<?>) call(ST + "pools.ListPoolElement", "getElements", element, new String[]{})) children.add(elementIdentity(child));
            return Map.of("kind", "list", "elements", children);
        }
        if (element instanceof fgq) {
            Object feature = invoke("instanceField", new Class[]{String.class, String.class, Object.class}, ST + "pools.FeaturePoolElement", "feature", element);
            return Map.of("kind", "feature", "feature", call(MC + "core.Holder", "getRegisteredName", feature, new String[]{}));
        }
        if (element instanceof fgp) return Map.of("kind", "empty");
        throw new IllegalArgumentException("unknown native pool element " + element.getClass());
    }

    static Object capture(Object nativeState, Object manager, Object bounds, long seed, String argument) throws Exception {
        String[] request = argument.split("@", 2), coordinate = request[1].split(":", 2);
        int cx = Integer.parseInt(coordinate[0]), cz = Integer.parseInt(coordinate[1]);
        Object lookup = component(nativeState, "lookup"), source = component(nativeState, "source"), random = component(nativeState, "random");
        Object structures = invoke("registry", new Class[]{Object.class, String.class}, lookup, "STRUCTURE");
        Object id = call(MC + "resources.Identifier", "parse", null, new String[]{"java.lang.String"}, request[0]);
        Object key = call(MC + "resources.ResourceKey", "create", null, new String[]{MC + "resources.ResourceKey", MC + "resources.Identifier"}, field(MC + "core.registries.Registries", "STRUCTURE"), id);
        Object holder = call(MC + "core.HolderGetter", "getOrThrow", structures, new String[]{MC + "resources.ResourceKey"}, key);
        Object structure = call(MC + "core.Holder", "value", holder, new String[]{});
        eqg generator = (eqg) make(LG + "NoiseBasedChunkGenerator", new String[]{MC + "world.level.biome.BiomeSource", MC + "core.Holder"}, source, component(nativeState, "settingsHolder"));
        Queries queries = new Queries(generator);
        Object chunk = make(MC + "world.level.ChunkPos", new String[]{"int", "int"}, cx, cz);
        Object context = make(ST + "Structure$GenerationContext", new String[]{MC + "core.RegistryAccess", MC + "world.level.chunk.ChunkGenerator", MC + "world.level.biome.BiomeSource", LG + "RandomState", ST + "templatesystem.StructureTemplateManager", "long", MC + "world.level.ChunkPos", MC + "world.level.LevelHeightAccessor", "java.util.function.Predicate"}, lookup, queries, source, random, manager, seed, chunk, bounds, (Predicate<Object>) value -> true);
        Optional<?> stub = (Optional<?>) call(ST + "Structure", "findGenerationPoint", structure, new String[]{ST + "Structure$GenerationContext"}, context);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("structure", request[0]); result.put("chunk", List.of(cx, cz)); result.put("present", stub.isPresent());
        List<Object> pieces = new ArrayList<>();
        if (stub.isPresent()) {
            Object position = call(ST + "Structure$GenerationStub", "position", stub.get(), new String[]{});
            result.put("position", position(position));
            List<Integer> coordinates = position(position);
            result.put("biome", invoke("biome", new Class[]{Object.class, Object.class, int.class, int.class, int.class}, source, component(nativeState, "sampler"), coordinates.get(0) >> 2, coordinates.get(1) >> 2, coordinates.get(2) >> 2));
            Object builder = call(ST + "Structure$GenerationStub", "getPiecesBuilder", stub.get(), new String[]{});
            Object container = call(ST + "pieces.StructurePiecesBuilder", "build", builder, new String[]{});
            for (Object piece : (List<?>) call(ST + "pieces.PiecesContainer", "pieces", container, new String[]{})) {
                Map<String, Object> row = new LinkedHashMap<>();
                row.put("bounds", box(call(ST + "StructurePiece", "getBoundingBox", piece, new String[]{})));
                row.put("depth", call(ST + "StructurePiece", "getGenDepth", piece, new String[]{}));
                if (piece instanceof ffk) {
                    Object element = call(ST + "PoolElementStructurePiece", "getElement", piece, new String[]{});
                    row.put("element", element.toString());
                    row.put("element_identity", elementIdentity(element));
                    row.put("projection", call(ST + "pools.StructurePoolElement", "getProjection", element, new String[]{}).toString());
                    row.put("position", position(call(ST + "PoolElementStructurePiece", "getPosition", piece, new String[]{})));
                    row.put("ground_level_delta", call(ST + "PoolElementStructurePiece", "getGroundLevelDelta", piece, new String[]{}));
                    row.put("rotation", call(ST + "PoolElementStructurePiece", "getRotation", piece, new String[]{}).toString());
                    List<Object> junctions = new ArrayList<>();
                    for (Object junction : (List<?>) call(ST + "PoolElementStructurePiece", "getJunctions", piece, new String[]{})) {
                        Map<String, Object> j = new LinkedHashMap<>();
                        for (String accessor : List.of("getSourceX", "getSourceGroundY", "getSourceZ", "getDeltaY", "getDestProjection")) j.put(accessor, call(ST + "pools.JigsawJunction", accessor, junction, new String[]{}));
                        junctions.add(j);
                    }
                    row.put("junctions", junctions);
                }
                if (piece instanceof ffy) {
                    row.put("template", call(ST + "TemplateStructurePiece", "makeTemplateLocation", piece, new String[]{}).toString());
                    row.put("position", position(call(ST + "TemplateStructurePiece", "templatePosition", piece, new String[]{})));
                    row.put("rotation", call(ST + "TemplateStructurePiece", "getRotation", piece, new String[]{}).toString());
                }
                pieces.add(row);
            }
        }
        result.put("pieces", pieces); result.put("height_queries", queries.heights); result.put("column_queries", queries.columns);
        return result;
    }
    static List<Object> paddingCases() throws Exception {
        List<Object> rows = new ArrayList<>();
        for (int[] level : List.of(new int[]{-64, 384}, new int[]{0, 256})) {
            Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null, new String[]{"int", "int"}, level[0], level[1]);
            Object padding = make(ST + "pools.DimensionPadding", new String[]{"int", "int"}, 10, 10);
            int bottom = level[0] + 10, top = level[0] + level[1] - 11;
            for (int[] ys : List.of(new int[]{bottom, bottom + 3}, new int[]{bottom - 1, bottom + 3}, new int[]{top - 3, top}, new int[]{top - 3, top + 1})) {
                Object box = make(ST + "BoundingBox", new String[]{"int", "int", "int", "int", "int", "int"}, 0, ys[0], 0, 4, ys[1], 4);
                Object rejected = call(ST + "pools.JigsawPlacement", "isStartTooCloseToWorldHeightLimits", null, new String[]{MC + "world.level.LevelHeightAccessor", ST + "pools.DimensionPadding", ST + "BoundingBox"}, bounds, padding, box);
                rows.add(Map.of("min_y", level[0], "level_height", level[1], "padding", List.of(10, 10), "bounds", box(box), "rejected", rejected));
            }
        }
        return rows;
    }

    static Object processorPrimitives() throws Exception {
        Object gravel = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "GRAVEL"), new String[]{});
        Object sand = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "SAND"), new String[]{});
        Object originalPos = make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, 0, 0, 0);
        Object currentPos = make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, -16, 41, 9);
        String info = ST + "templatesystem.StructureTemplate$StructureBlockInfo";
        Object original = make(info, new String[]{MC + "core.BlockPos", MC + "world.level.block.state.BlockState", MC + "nbt.CompoundTag"}, originalPos, gravel, null);
        Object current = make(info, new String[]{MC + "core.BlockPos", MC + "world.level.block.state.BlockState", MC + "nbt.CompoundTag"}, currentPos, sand, null);
        Object settings = make(ST + "templatesystem.StructurePlaceSettings", new String[]{});
        Object delegate = make(ST + "templatesystem.BlockRotProcessor", new String[]{"float"}, 0.0f);
        List<Object> capped = new ArrayList<>();
        for (int limit : new int[]{0, 2, 3, 6, 20}) {
            Object count = call(MC + "util.valueproviders.ConstantInt", "of", null, new String[]{"int"}, limit);
            Object processor = make(ST + "templatesystem.CappedProcessor", new String[]{ST + "templatesystem.StructureProcessor", MC + "util.valueproviders.IntProvider"}, delegate, count);
            Object result = call(ST + "templatesystem.StructureProcessor", "processBlock", processor,
                new String[]{MC + "world.level.LevelReader", MC + "core.BlockPos", MC + "core.BlockPos", info, info, ST + "templatesystem.StructurePlaceSettings"},
                null, originalPos, originalPos, original, current, settings);
            capped.add(Map.of("limit", limit, "same_current_object", result == current, "same_original_object", result == original,
                "state", state(call(info, "state", result, new String[]{})), "position", position(call(info, "pos", result, new String[]{}))));
        }
        List<Object> references = new ArrayList<>();
        for (int count : new int[]{0, 1, 2, 3, 24, 25, 48, 49, 100}) {
            var nativeSet = new it.unimi.dsi.fastutil.longs.LongOpenHashSet();
            List<Long> insertion = new ArrayList<>();
            if (count > 0) { insertion.add(0L); nativeSet.add(0L); }
            for (int i = 1; i < count; ++i) {
                int x = i / 13 - 6, z = i % 13 - 6;
                long packed = (x & 0xffffffffL) | ((z & 0xffffffffL) << 32);
                if (packed == 0) packed = Long.MIN_VALUE;
                insertion.add(packed); nativeSet.add(packed);
            }
            references.add(Map.of("insertion", insertion, "native_iteration", nativeSet.toLongArray()));
        }
        return Map.of("capped_per_block", capped, "reference_sets", references);
    }

    static List<Object> shapeCases(Path input) throws Exception {
        var requests = com.google.gson.JsonParser.parseString(Files.readString(input)).getAsJsonArray();
        Object registry = field(MC + "world.level.block.Block", "BLOCK_STATE_REGISTRY");
        Object air = call(MC + "core.IdMapper", "byId", registry, new String[]{"int"}, 0);
        Method getState = (Method) invoke("method", new Class[]{String.class, String.class, String[].class},
            MC + "world.level.BlockGetter", "getBlockState", new String[]{MC + "core.BlockPos"});
        Method getFluid = (Method) invoke("method", new Class[]{String.class, String.class, String[].class},
            MC + "world.level.BlockGetter", "getFluidState", new String[]{MC + "core.BlockPos"});
        Class<?> readerType = (Class<?>) invoke("type", new Class[]{String.class}, MC + "world.level.LevelReader");
        Class<?> ticksType = (Class<?>) invoke("type", new Class[]{String.class}, MC + "world.level.ScheduledTickAccess");
        Method update = (Method) invoke("method", new Class[]{String.class, String.class, String[].class},
            MC + "world.level.block.state.BlockBehaviour$BlockStateBase", "updateShape",
            new String[]{MC + "world.level.LevelReader", MC + "world.level.ScheduledTickAccess", MC + "core.BlockPos", MC + "core.Direction", MC + "core.BlockPos", MC + "world.level.block.state.BlockState", MC + "util.RandomSource"});
        List<Object> rows = new ArrayList<>();
        for (var request : requests) {
            var data = request.getAsJsonObject();
            Object pos = make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, 0, 64, 0);
            Object nativeState = call(MC + "core.IdMapper", "byId", registry, new String[]{"int"}, data.get("state_id").getAsInt());
            Map<Object, Object> world = new HashMap<>(); world.put(pos, nativeState);
            List<Object> neighbours = new ArrayList<>();
            for (var entry : data.getAsJsonArray("world")) {
                var row = entry.getAsJsonObject(); var xyz = row.getAsJsonArray("pos");
                Object p = make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, xyz.get(0).getAsInt(), xyz.get(1).getAsInt(), xyz.get(2).getAsInt());
                Object value = call(MC + "core.IdMapper", "byId", registry, new String[]{"int"}, row.get("state_id").getAsInt());
                world.put(p, value); neighbours.add(Map.of("pos", position(p), "state", state(value)));
            }
            List<Object> scheduled = new ArrayList<>();
            Object reader = Proxy.newProxyInstance(readerType.getClassLoader(), new Class[]{readerType}, (proxy, method, args) -> {
                if (method.getName().equals(getState.getName()) && args != null && args.length == 1) return world.getOrDefault(args[0], air);
                if (method.getName().equals(getFluid.getName()) && args != null && args.length == 1) return call(MC + "world.level.block.state.BlockBehaviour$BlockStateBase", "getFluidState", world.getOrDefault(args[0], air), new String[]{});
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, args == null ? new Object[]{} : args);
                throw new IllegalStateException("uncaptured native shape world query " + method);
            });
            Object ticks = Proxy.newProxyInstance(ticksType.getClassLoader(), new Class[]{ticksType}, (proxy, method, args) -> {
                if (method.getReturnType() == void.class && args != null && args.length >= 3 && args[0] instanceof is) {
                    scheduled.add(Map.of("pos", position(args[0]), "delay", args[2], "type", args[1].getClass().getName())); return null;
                }
                throw new IllegalStateException("unexpected native scheduled-tick query " + method);
            });
            Object direction = field(MC + "core.Direction", data.get("direction").getAsString());
            Object npos = call(MC + "core.BlockPos", "relative", pos, new String[]{MC + "core.Direction"}, direction);
            Object random = call(MC + "util.RandomSource", "create", null, new String[]{"long"}, 0L);
            Object result = update.invoke(nativeState, reader, ticks, pos, direction, npos, world.getOrDefault(npos, air), random);
            rows.add(Map.of("input", state(nativeState), "position", position(pos), "direction", data.get("direction").getAsString(),
                "world", neighbours, "expected", state(result), "scheduled_native_only", scheduled));
        }
        return rows;
    }

    /** Executes the genuine spread/drip consumer against an explicit flat block accessor. */
    static List<Object> portalSpreadCases(Object manager) throws Exception {
        String owner = ST + "structures.RuinedPortalPiece";
        Object id = call(MC + "resources.Identifier", "parse", null, new String[]{"java.lang.String"}, "minecraft:ruined_portal/portal_1");
        Object template = call(ST + "templatesystem.StructureTemplateManager", "getOrCreate", manager, new String[]{MC + "resources.Identifier"}, id);
        Object air = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "AIR"), new String[]{});
        Method getState = (Method) invoke("method", new Class[]{String.class, String.class, String[].class},
            MC + "world.level.BlockGetter", "getBlockState", new String[]{MC + "core.BlockPos"});
        Method getHeight = (Method) invoke("method", new Class[]{String.class, String.class, String[].class},
            MC + "world.level.LevelReader", "getHeight", new String[]{LG + "Heightmap$Types", "int", "int"});
        Method setBlock = (Method) invoke("method", new Class[]{String.class, String.class, String[].class},
            MC + "world.level.LevelWriter", "setBlock", new String[]{MC + "core.BlockPos", MC + "world.level.block.state.BlockState", "int", "int"});
        Class<?> accessor = (Class<?>) invoke("type", new Class[]{String.class}, MC + "world.level.LevelAccessor");
        List<Object> rows = new ArrayList<>();
        for (String mode : List.of("ON_LAND_SURFACE", "ON_LAND_SURFACE_COLD", "ON_OCEAN_FLOOR", "UNDERGROUND", "IN_NETHER")) {
            boolean cold = mode.endsWith("_COLD"), overgrown = mode.equals("ON_OCEAN_FLOOR");
            String placement = cold ? "ON_LAND_SURFACE" : mode;
            int pieceY = placement.equals("UNDERGROUND") ? 68 : 69;
            Object fill = call(MC + "world.level.block.Block", "defaultBlockState",
                field(MC + "world.level.block.Blocks", placement.equals("IN_NETHER") ? "LAVA" : "STONE"), new String[]{});
            Object water = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "WATER"), new String[]{});
            Map<List<Integer>, Object> blocks = new HashMap<>();
            java.util.function.Function<List<Integer>, Object> read = p -> blocks.getOrDefault(p,
                p.get(1) < 70 ? fill : overgrown && p.get(1) < 73 ? water : air);
            int[] heights = {0};
            List<Object> writes = new ArrayList<>();
            Object world = Proxy.newProxyInstance(accessor.getClassLoader(), new Class[]{accessor}, (proxy, method, args) -> {
                if (method.getName().equals(getState.getName()) && args != null && args.length == 1) return read.apply(position(args[0]));
                if (method.getName().equals(getHeight.getName()) && args != null && args.length == 3 && args[0] instanceof euq.a) {
                    heights[0]++;
                    Predicate<Object> opaque = (Predicate<Object>) call(LG + "Heightmap$Types", "isOpaque", args[0], new String[]{});
                    for (int y = 319; y >= -64; --y) if (opaque.test(read.apply(List.of((int) args[1], y, (int) args[2])))) return y + 1;
                    return -64;
                }
                if (method.getName().equals(setBlock.getName()) && args != null && args.length >= 3 && args[0] instanceof is) {
                    List<Integer> p = position(args[0]);
                    blocks.put(p, args[1]); writes.add(Map.of("pos", p, "state", state(args[1]))); return true;
                }
                throw new IllegalStateException("uncaptured native portal world query " + method);
            });
            Object pos = make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, 0, pieceY, 0);
            Object pivot = make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, 0, 0, 0);
            Object properties = make(owner + "$Properties", new String[]{"boolean", "float", "boolean", "boolean", "boolean", "boolean"},
                cold, 0.2f, false, overgrown, false, false);
            Object piece = make(owner, new String[]{ST + "templatesystem.StructureTemplateManager", MC + "core.BlockPos", owner + "$VerticalPlacement",
                owner + "$Properties", MC + "resources.Identifier", ST + "templatesystem.StructureTemplate", MC + "world.level.block.Rotation", MC + "world.level.block.Mirror", MC + "core.BlockPos"},
                manager, pos, field(owner + "$VerticalPlacement", placement), properties, id, template,
                field(MC + "world.level.block.Rotation", "NONE"), field(MC + "world.level.block.Mirror", "NONE"), pivot);
            Object random = make(LG + "WorldgenRandom", new String[]{MC + "util.RandomSource"},
                make(LG + "XoroshiroRandomSource", new String[]{"long"}, 11L));
            call(owner, "spreadNetherrack", piece, new String[]{MC + "util.RandomSource", MC + "world.level.LevelAccessor"}, random, world);
            List<List<Integer>> positions = new ArrayList<>(blocks.keySet());
            positions.sort(Comparator.<List<Integer>, Integer>comparing(p -> p.get(0)).thenComparing(p -> p.get(1)).thenComparing(p -> p.get(2)));
            List<Object> expected = new ArrayList<>();
            for (List<Integer> p : positions) expected.add(Map.of("pos", p, "state", state(blocks.get(p))));
            Map<String, Object> row = new LinkedHashMap<>();
            row.put("mode", mode); row.put("placement", placement.toLowerCase(Locale.ROOT)); row.put("cold", cold); row.put("overgrown", overgrown);
            row.put("seed", 11L); row.put("template", id.toString()); row.put("position", position(pos)); row.put("pivot", position(pivot));
            row.put("bounds", box(call(ST + "StructurePiece", "getBoundingBox", piece, new String[]{})));
            row.put("fill_y", 70); row.put("fill", state(fill)); row.put("water_top_exclusive", overgrown ? 73 : 70);
            row.put("height_queries", heights[0]); row.put("ordered_writes", writes); row.put("blocks", expected);
            row.put("next_long", call(MC + "util.RandomSource", "nextLong", random, new String[]{})); rows.add(row);
        }
        return rows;
    }


    static String sha(Path path) throws Exception { return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(path))); }
    @SuppressWarnings("removal")
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]);
        List<String> networkAttempts = Collections.synchronizedList(new ArrayList<>());
        System.setSecurityManager(new SecurityManager() {
            public void checkPermission(java.security.Permission permission) {}
            private void deny(String operation) { networkAttempts.add(operation); throw new SecurityException("network forbidden: " + operation); }
            public void checkConnect(String host, int port) { deny("connect " + host + ":" + port); }
            public void checkListen(int port) { deny("listen " + port); }
            public void checkAccept(String host, int port) { deny("accept " + host + ":" + port); }
        });
        var factory = (org.apache.logging.log4j.core.impl.Log4jContextFactory) org.apache.logging.log4j.LogManager.getFactory();
        var loggingContext = factory.getSelector().getContext("NativeStructureOracle", ClassLoader.getSystemClassLoader(), false);
        var logging = new org.apache.logging.log4j.core.config.DefaultConfiguration();
        ((Map<String, String>) logging.getComponent("ContextProperties")).put("hostName", "localhost");
        loggingContext.start(logging);
        Class<?> type = Class.forName("NativeWorldgenOracle");
        Constructor<?> constructor = type.getDeclaredConstructor(Path.class); constructor.setAccessible(true);
        oracle = constructor.newInstance(root.resolve("server-mappings.txt"));
        for (String[] pin : List.of(new String[]{"server.jar", "64bb6d763bed0a9f1d632ec347938594144943ed", "SHA-1"}, new String[]{"server-mappings.txt", "5621e9253f05fd57872bbe7f8ddf5f9a7d525955", "SHA-1"}, new String[]{"versions/1.21.11/server-1.21.11.jar", "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc", "SHA-256"})) invoke("verify", new Class[]{Path.class, String.class, String.class}, root.resolve(pin[0]), pin[1], pin[2]);
        long seed = Long.parseLong(args[2]); String dimension = args[3];
        Object nativeState = invoke("bootstrap", new Class[]{long.class, String.class, boolean.class}, seed, dimension, true);
        if (args.length > 5 && args[4].equals("--shape-cases")) {
            Map<String, Object> output = new LinkedHashMap<>();
            output.put("schema", "native-shape-consumers-v1"); output.put("version", "1.21.11");
            output.put("source_sha256", sha(Path.of("crates/unastar_noise/tools/NativeStructureOracle.java")));
            output.put("inner_sha256", sha(root.resolve("versions/1.21.11/server-1.21.11.jar"))); output.put("invocation", List.of(args));
            output.put("input_sha256", sha(Path.of(args[5]))); output.put("denied_network_attempts", networkAttempts);
            output.put("server_constructed", false); output.put("server_ticks", 0); output.put("cases", shapeCases(Path.of(args[5])));
            Files.writeString(Path.of(args[1]), new Gson().toJson(output)); System.exit(0);
        }
        if (args.length > 4 && args[4].equals("--processor-primitives")) {
            Map<String, Object> output = new LinkedHashMap<>();
            output.put("version", "1.21.11"); output.put("source_sha256", sha(Path.of("crates/unastar_noise/tools/NativeStructureOracle.java")));
            output.put("inner_sha256", sha(root.resolve("versions/1.21.11/server-1.21.11.jar")));
            output.put("denied_network_attempts", networkAttempts); output.put("server_constructed", false); output.put("server_ticks", 0);
            output.put("invocation", List.of(args)); output.put("result", processorPrimitives());
            Files.writeString(Path.of(args[1]), new Gson().toJson(output));
            System.exit(0);
        }
        Path work = Files.createTempDirectory(root, "native-structure-queries-");
        Object storageSource = call(MC + "world.level.storage.LevelStorageSource", "createDefault", null, new String[]{"java.nio.file.Path"}, work);
        Object storage = call(MC + "world.level.storage.LevelStorageSource", "createAccess", storageSource, new String[]{"java.lang.String"}, "world");
        Object pack = call(MC + "server.packs.repository.ServerPacksSource", "createVanillaPackSource", null, new String[]{});
        Object resources = make(MC + "server.packs.resources.MultiPackResourceManager", new String[]{MC + "server.packs.PackType", "java.util.List"}, field(MC + "server.packs.PackType", "SERVER_DATA"), List.of(pack));
        try {
            Object lookup = component(nativeState, "lookup");
            Object blocks = call(MC + "core.HolderLookup$Provider", "lookupOrThrow", call(MC + "core.RegistryAccess", "fromRegistryOfRegistries", null, new String[]{MC + "core.Registry"}, field(MC + "core.registries.BuiltInRegistries", "REGISTRY")), new String[]{MC + "resources.ResourceKey"}, field(MC + "core.registries.Registries", "BLOCK"));
            Object manager = make(ST + "templatesystem.StructureTemplateManager", new String[]{MC + "server.packs.resources.ResourceManager", MC + "world.level.storage.LevelStorageSource$LevelStorageAccess", "com.mojang.datafixers.DataFixer", MC + "core.HolderGetter"}, resources, storage, call(MC + "util.datafix.DataFixers", "getDataFixer", null, new String[]{}), blocks);
            if (args.length > 4 && args[4].equals("--portal-spread")) {
                Map<String, Object> output = new LinkedHashMap<>();
                output.put("schema", "native-portal-spread-v1"); output.put("version", "1.21.11");
                output.put("source_sha256", sha(Path.of("crates/unastar_noise/tools/NativeStructureOracle.java")));
                output.put("inner_sha256", sha(root.resolve("versions/1.21.11/server-1.21.11.jar"))); output.put("invocation", List.of(args));
                output.put("profile", "actual RuinedPortalPiece.spreadNetherrack with native random and native heightmap predicates; flat explicit proxy accessor; no server/level/chunk-stage/ticks/listeners");
                output.put("denied_network_attempts", networkAttempts); output.put("server_constructed", false); output.put("server_ticks", 0);
                output.put("cases", portalSpreadCases(manager));
                output.put("natural_start", capture(nativeState, manager, call(MC + "world.level.LevelHeightAccessor", "create", null, new String[]{"int", "int"}, -64, 384), seed, "minecraft:ruined_portal@1:0"));
                Files.writeString(Path.of(args[1]), new Gson().toJson(output)); return;
            }
            int min = dimension.equals("overworld") ? -64 : 0, height = dimension.equals("overworld") ? 384 : 256;
            Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null, new String[]{"int", "int"}, min, height);
            List<Object> cases = new ArrayList<>();
            for (int i = 4; i < args.length; ++i) cases.add(capture(nativeState, manager, bounds, seed, args[i]));
            Map<String, Object> output = new LinkedHashMap<>();
            output.put("version", "1.21.11"); output.put("seed", seed); output.put("dimension", dimension); output.put("min_y", min); output.put("level_height", height); output.put("generator_height", ((eqg) make(LG + "NoiseBasedChunkGenerator", new String[]{MC + "world.level.biome.BiomeSource", MC + "core.Holder"}, component(nativeState, "source"), component(nativeState, "settingsHolder"))).e());
            output.put("profile", "actual native Structure.findGenerationPoint and stub builder with delegating genuine generator; no server/level/chunk-stage/ticks/listeners");
            output.put("denied_network_attempts", networkAttempts); output.put("server_ticks", 0); output.put("server_constructed", false);
            output.put("padding_cases", paddingCases());
            output.put("source_sha256", sha(Path.of("crates/unastar_noise/tools/NativeStructureOracle.java")));
            output.put("inner_sha256", sha(root.resolve("versions/1.21.11/server-1.21.11.jar"))); output.put("invocation", List.of(args)); output.put("cases", cases);
            Files.writeString(Path.of(args[1]), new Gson().toJson(output));
        } finally {
            call(MC + "server.packs.resources.MultiPackResourceManager", "close", resources, new String[]{});
            call(MC + "world.level.storage.LevelStorageSource$LevelStorageAccess", "close", storage, new String[]{});
        }
        System.exit(0);
    }
}
