/* Pinned Java 1.21.11 reference-only geometry/density fixture capture.
 * Starts no server, opens no sockets. Run JDK21 with -XX:ActiveProcessorCount=1
 * -Xmx1G, bundled inner server + libraries on the classpath. Arguments:
 * <external-runtime-root> <output.json>. No native worldgen/RNG parity claim:
 * these are deliberately constructed native pieces and reference maps, passed
 * through the real StructureManager and Beardifier.forStructuresInChunk.
 */
package nativebeardifier;

import com.google.gson.Gson;
import java.lang.reflect.*;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.stream.Stream;

public final class NativeBeardifierOracle {
    private static final String MC = "net.minecraft.";
    private static final String LG = MC + "world.level.levelgen.";
    private static final String ST = LG + "structure.";
    private static final String CH = MC + "world.level.chunk.";
    private static final String BB = ST + "BoundingBox";
    private static final String POS = MC + "core.BlockPos";
    private static final String PROJECTION = ST + "pools.StructureTemplatePool$Projection";
    private static final String ADJUSTMENT = ST + "TerrainAdjustment";
    private static final String BEARD = LG + "Beardifier";
    private static final String CONTEXT = LG + "DensityFunction$FunctionContext";
    private final Map<String, String> classes = new HashMap<>();
    private final Map<String, Map<String, String>> members = new HashMap<>();

    private NativeBeardifierOracle(Path mappings) throws Exception {
        String owner = null;
        for (String line : Files.readAllLines(mappings)) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] parts = line.trim().split(" -> ", 2);
            if (parts.length != 2) continue;
            if (!Character.isWhitespace(line.charAt(0))) {
                owner = parts[0];
                classes.put(owner, parts[1].substring(0, parts[1].length() - 1));
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

    private Class<?> type(String name) throws Exception {
        return switch (name) {
            case "int" -> int.class;
            case "long" -> long.class;
            case "double" -> double.class;
            default -> Class.forName(classes.getOrDefault(name, name));
        };
    }

    private Class<?>[] types(String... names) throws Exception {
        Class<?>[] result = new Class<?>[names.length];
        for (int i = 0; i < names.length; i++) result[i] = type(names[i]);
        return result;
    }

    private Method method(String owner, String name, String... parameters) throws Exception {
        String mapped = owner.startsWith(MC)
            ? Objects.requireNonNull(members.get(owner).get(name + "(" + String.join(",", parameters) + ")"), owner + "." + name)
            : name;
        Method method;
        try { method = type(owner).getMethod(mapped, types(parameters)); }
        catch (NoSuchMethodException e) { method = type(owner).getDeclaredMethod(mapped, types(parameters)); }
        method.setAccessible(true);
        return method;
    }

    private Object call(String owner, String name, Object receiver, String[] parameters, Object... args) throws Exception {
        return method(owner, name, parameters).invoke(receiver, args);
    }

    private Object construct(String owner, String[] parameters, Object... args) throws Exception {
        Constructor<?> constructor = type(owner).getDeclaredConstructor(types(parameters));
        constructor.setAccessible(true);
        return constructor.newInstance(args);
    }

    private Field field(String owner, String name) throws Exception {
        Field field = type(owner).getDeclaredField(Objects.requireNonNull(members.get(owner).get(name)));
        field.setAccessible(true);
        return field;
    }

    private Object constant(String owner, String name) throws Exception { return field(owner, name).get(null); }
    private Object pos(int x, int y, int z) throws Exception { return construct(POS, new String[]{"int", "int", "int"}, x, y, z); }
    private Object box(int[] b) throws Exception {
        return construct(BB, new String[]{"int", "int", "int", "int", "int", "int"}, b[0], b[1], b[2], b[3], b[4], b[5]);
    }
    private int[] boxValues(Object box) throws Exception {
        if (box == null) return null;
        int[] result = new int[6];
        String[] getters = {"minX", "minY", "minZ", "maxX", "maxY", "maxZ"};
        for (int i = 0; i < 6; i++) result[i] = (int) call(BB, getters[i], box, new String[]{});
        return result;
    }

    private Object paletteFactory() throws Exception {
        Object lookup = call(MC + "data.registries.VanillaRegistries", "createLookup", null, new String[]{});
        Object biomes = call(MC + "core.HolderLookup$Provider", "lookupOrThrow", lookup,
            new String[]{MC + "resources.ResourceKey"}, constant(MC + "core.registries.Registries", "BIOME"));
        List<?> holders = ((Stream<?>) call(MC + "core.HolderLookup", "listElements", biomes, new String[]{})).toList();
        Object ids = construct(MC + "core.IdMapper", new String[]{});
        for (Object holder : holders) call(MC + "core.IdMapper", "add", ids, new String[]{"java.lang.Object"}, holder);
        Object biomeStrategy = call(CH + "Strategy", "createForBiomes", null, new String[]{MC + "core.IdMap"}, ids);
        Object blockStrategy = call(CH + "Strategy", "createForBlockStates", null, new String[]{MC + "core.IdMap"},
            constant(MC + "world.level.block.Block", "BLOCK_STATE_REGISTRY"));
        Object air = call(MC + "world.level.block.Block", "defaultBlockState",
            constant(MC + "world.level.block.Blocks", "AIR"), new String[]{});
        String codec = "com.mojang.serialization.Codec";
        Object biomeCodec = call(MC + "resources.RegistryFixedCodec", "create", null,
            new String[]{MC + "resources.ResourceKey"}, constant(MC + "core.registries.Registries", "BIOME"));
        Object statesCodec = call(CH + "PalettedContainer", "codecRW", null,
            new String[]{codec, CH + "Strategy", "java.lang.Object"}, constant(MC + "world.level.block.state.BlockState", "CODEC"), blockStrategy, air);
        Object biomesCodec = call(CH + "PalettedContainer", "codecRO", null,
            new String[]{codec, CH + "Strategy", "java.lang.Object"}, biomeCodec, biomeStrategy, holders.getFirst());
        return construct(CH + "PalettedContainerFactory",
            new String[]{CH + "Strategy", MC + "world.level.block.state.BlockState", codec, CH + "Strategy", MC + "core.Holder", codec},
            blockStrategy, air, statesCodec, biomeStrategy, holders.getFirst(), biomesCodec);
    }

    private record PieceInput(String kind, int[] box, String projection, int groundDelta, List<int[]> junctions) {}

    private Object nativePiece(PieceInput input) throws Exception {
        int[] b = input.box;
        Object piece;
        if (input.kind.equals("ordinary")) {
            piece = construct(ST + "structures.BuriedTreasurePieces$BuriedTreasurePiece", new String[]{POS}, pos(b[0], b[1], b[2]));
            field(ST + "StructurePiece", "boundingBox").set(piece, box(b));
        } else {
            Object projection = constant(PROJECTION, input.projection);
            Object element = construct(ST + "pools.FeaturePoolElement", new String[]{MC + "core.Holder", PROJECTION}, null, projection);
            piece = construct(ST + "PoolElementStructurePiece",
                new String[]{ST + "templatesystem.StructureTemplateManager", ST + "pools.StructurePoolElement", POS, "int",
                    MC + "world.level.block.Rotation", BB, ST + "templatesystem.LiquidSettings"},
                null, element, pos(b[0], b[1], b[2]), input.groundDelta,
                constant(MC + "world.level.block.Rotation", "NONE"), box(b), constant(ST + "templatesystem.LiquidSettings", "APPLY_WATERLOGGING"));
            for (int[] j : input.junctions) {
                Object junction = construct(ST + "pools.JigsawJunction", new String[]{"int", "int", "int", "int", PROJECTION},
                    j[0], j[1], j[2], j[3], constant(PROJECTION, j[4] == 0 ? "TERRAIN_MATCHING" : "RIGID"));
                call(ST + "PoolElementStructurePiece", "addJunction", piece, new String[]{ST + "pools.JigsawJunction"}, junction);
            }
        }
        return piece;
    }

    private Object fixture(Object factory, String adjustment, int cx, int cz, int y) throws Exception {
        int x = cx * 16, z = cz * 16;
        List<PieceInput> inputs = List.of(
            new PieceInput("ordinary", new int[]{x-4,y,z+2,x+7,y+8,z+11}, null, 0, List.of()),
            new PieceInput("pool", new int[]{x+9,y-5,z-2,x+19,y+13,z+8}, "RIGID", -3,
                List.of(new int[]{x+8,y-8,z+4,7,0}, new int[]{x+26,y+1,z+26,-4,1})),
            new PieceInput("pool", new int[]{x-12,y+3,z+12,x-7,y+8,z+17}, "TERRAIN_MATCHING", 6,
                List.of(new int[]{x-12,y+4,z,3,1}, new int[]{x-11,y-2,z+3,-5,0}, new int[]{x+27,y,z,0,1},
                    new int[]{x,y+2,z-12,1,1}, new int[]{x,y+2,z+27,1,0}, new int[]{x,y+2,z-11,1,1})),
            new PieceInput("ordinary", new int[]{x+27,y+2,z+1,x+29,y+3,z+2}, null, 0, List.of()),
            new PieceInput("ordinary", new int[]{x+28,y+30,z,x+30,y+32,z+2}, null, 0, List.of()),
            new PieceInput("pool", new int[]{x-15,y-3,z,x-13,y+4,z+3}, "RIGID", 11,
                List.of(new int[]{x,y,z,0,1})),
            // A real pool-piece ground delta may extend beyond its box. This
            // deliberately tests 1.21.11 affected-box clipping, not just zeros
            // outside the compact kernel.
            new PieceInput("pool", new int[]{x+1,y+5,z+14,x+3,y+9,z+18}, "RIGID", 47, List.of()));
        List<Object> pieces = new ArrayList<>();
        for (PieceInput input : inputs) pieces.add(nativePiece(input));
        Object settings = construct(ST + "Structure$StructureSettings",
            new String[]{MC + "core.HolderSet", "java.util.Map", LG + "GenerationStep$Decoration", ADJUSTMENT},
            call(MC + "core.HolderSet", "direct", null, new String[]{"java.util.List"}, List.of()),
            Map.of(), constant(LG + "GenerationStep$Decoration", "SURFACE_STRUCTURES"), constant(ADJUSTMENT, adjustment));
        Object structure = construct(ST + "structures.BuriedTreasureStructure", new String[]{ST + "Structure$StructureSettings"}, settings);
        Object chunkPos = construct(MC + "world.level.ChunkPos", new String[]{"int", "int"}, cx, cz);
        Object start = construct(ST + "StructureStart",
            new String[]{ST + "Structure", MC + "world.level.ChunkPos", "int", ST + "pieces.PiecesContainer"},
            structure, chunkPos, 0, construct(ST + "pieces.PiecesContainer", new String[]{"java.util.List"}, pieces));
        Object bounds = call(MC + "world.level.LevelHeightAccessor", "create", null, new String[]{"int", "int"}, -64, 384);
        Object chunk = construct(CH + "ProtoChunk",
            new String[]{MC + "world.level.ChunkPos", CH + "UpgradeData", MC + "world.level.LevelHeightAccessor",
                CH + "PalettedContainerFactory", LG + "blending.BlendingData"},
            chunkPos, constant(CH + "UpgradeData", "EMPTY"), bounds, factory, null);
        Object refs = construct("it.unimi.dsi.fastutil.longs.LongOpenHashSet", new String[]{});
        call("it.unimi.dsi.fastutil.longs.LongOpenHashSet", "add", refs, new String[]{"long"},
            call(MC + "world.level.ChunkPos", "toLong", chunkPos, new String[]{}));
        call(CH + "ChunkAccess", "setAllReferences", chunk, new String[]{"java.util.Map"}, Map.of(structure, refs));
        call(CH + "ProtoChunk", "setStartForStructure", chunk, new String[]{ST + "Structure", ST + "StructureStart"}, structure, start);
        String chunkMethod = method(MC + "world.level.LevelReader", "getChunk", "int", "int", CH + "status.ChunkStatus").getName();
        String minSectionMethod = method(MC + "world.level.LevelHeightAccessor", "getMinSectionY").getName();
        Object level = Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{type(MC + "world.level.LevelAccessor")},
            (proxy, method, args) -> {
                if (method.getName().equals(chunkMethod) && args != null && args.length == 3) return chunk;
                if (method.getName().equals(minSectionMethod) && method.getParameterCount() == 0) return -4;
                throw new UnsupportedOperationException("unexpected fixture world access: " + method);
            });
        Object manager = construct(MC + "world.level.StructureManager",
            new String[]{MC + "world.level.LevelAccessor", LG + "WorldOptions", ST + "StructureCheck"}, level, null, null);
        Object beard = call(BEARD, "forStructuresInChunk", null,
            new String[]{MC + "world.level.StructureManager", MC + "world.level.ChunkPos"}, manager, chunkPos);
        List<Object> collected = new ArrayList<>();
        for (Object rigid : (List<?>) field(BEARD, "pieces").get(beard)) {
            collected.add(Map.of("box", boxValues(call(BEARD + "$Rigid", "box", rigid, new String[]{})),
                "adjustment", ((Enum<?>) call(BEARD + "$Rigid", "terrainAdjustment", rigid, new String[]{})).name(),
                "ground_delta", call(BEARD + "$Rigid", "groundLevelDelta", rigid, new String[]{})));
        }
        List<int[]> junctions = new ArrayList<>();
        for (Object j : (List<?>) field(BEARD, "junctions").get(beard)) {
            junctions.add(new int[]{(int) call(ST + "pools.JigsawJunction", "getSourceX", j, new String[]{}),
                (int) call(ST + "pools.JigsawJunction", "getSourceGroundY", j, new String[]{}),
                (int) call(ST + "pools.JigsawJunction", "getSourceZ", j, new String[]{}),
                (int) call(ST + "pools.JigsawJunction", "getDeltaY", j, new String[]{}),
                ((Enum<?>) call(ST + "pools.JigsawJunction", "getDestProjection", j, new String[]{})).name().equals("RIGID") ? 1 : 0});
        }
        int[][] xz = {{0,0},{-4,2},{7,11},{9,-2},{19,8},{-11,3},{26,26},{27,1},{-12,0},{28,1},{-29,0},{53,0},{1,14}};
        int[] ys = {-40,-30,-25,-24,-20,-14,-13,-12,-11,-9,-8,-7,-5,-4,-3,-2,-1,0,1,2,3,4,5,7,8,9,11,12,13,14,20,24,25,37,38,40,51,52,53,54};
        List<double[]> samples = new ArrayList<>();
        for (int[] at : xz) for (int dy : ys) {
            int sx = x + at[0], sy = y + dy, sz = z + at[1];
            Object context = construct(LG + "DensityFunction$SinglePointContext", new String[]{"int", "int", "int"}, sx, sy, sz);
            samples.add(new double[]{sx,sy,sz,(double) call(BEARD, "compute", beard, new String[]{CONTEXT}, context)});
        }
        Map<String, Object> row = new LinkedHashMap<>();
        row.put("adjustment", adjustment); row.put("chunk", new int[]{cx, cz}); row.put("inputs", inputs);
        row.put("pieces", collected); row.put("junctions", junctions);
        row.put("affected_box", boxValues(field(BEARD, "affectedBox").get(beard))); row.put("samples", samples);
        return row;
    }

    private Object primitives() throws Exception {
        List<double[]> beard = new ArrayList<>(), bury = new ArrayList<>();
        for (int x : new int[]{-13,-12,-11,-1,0,1,11,12}) for (int y : new int[]{-13,-12,-1,0,1,11,12})
            for (int z : new int[]{-12,0,11,12}) for (int ground : new int[]{-30,-3,0,5,30})
                beard.add(new double[]{x,y,z,ground,(double) call(BEARD,"getBeardContribution",null,
                    new String[]{"int","int","int","int"},x,y,z,ground)});
        for (double x : new double[]{-6,-1,0,0.5,3,6,6.5}) for (double y : new double[]{-5,-0.5,0,2,6})
            for (double z : new double[]{0,1,5}) bury.add(new double[]{x,y,z,(double) call(BEARD,"getBuryContribution",null,
                new String[]{"double","double","double"},x,y,z)});
        return Map.of("beard",beard,"bury",bury);
    }

    private static void verify(Path path, String expected, String algorithm) throws Exception {
        MessageDigest digest = MessageDigest.getInstance(algorithm);
        try (var input = Files.newInputStream(path)) {
            byte[] bytes = new byte[65536]; int count;
            while ((count = input.read(bytes)) != -1) digest.update(bytes,0,count);
        }
        String actual = HexFormat.of().formatHex(digest.digest());
        if (!actual.equals(expected)) throw new IllegalArgumentException(path + ": unexpected " + algorithm + " " + actual);
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 2) throw new IllegalArgumentException("<external-runtime-root> <output.json>");
        Path root = Path.of(args[0]);
        verify(root.resolve("server.jar"), "64bb6d763bed0a9f1d632ec347938594144943ed", "SHA-1");
        verify(root.resolve("server-mappings.txt"), "5621e9253f05fd57872bbe7f8ddf5f9a7d525955", "SHA-1");
        verify(root.resolve("versions/1.21.11/server-1.21.11.jar"), "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc", "SHA-256");
        NativeBeardifierOracle oracle = new NativeBeardifierOracle(root.resolve("server-mappings.txt"));
        oracle.call(MC + "SharedConstants", "tryDetectVersion", null, new String[]{});
        oracle.call(MC + "server.Bootstrap", "bootStrap", null, new String[]{});
        Object factory = oracle.paletteFactory();
        List<Object> cases = new ArrayList<>();
        for (String adjustment : new String[]{"NONE","BURY","BEARD_THIN","BEARD_BOX","ENCAPSULATE"}) {
            cases.add(oracle.fixture(factory,adjustment,0,0,-60));
            cases.add(oracle.fixture(factory,adjustment,-2,1,64));
            cases.add(oracle.fixture(factory,adjustment,1,-2,-7));
        }
        Map<String,Object> output = new LinkedHashMap<>();
        output.put("version","1.21.11");
        output.put("provenance","Pinned native APIs; deliberately constructed geometry, real native StructureManager reference lookup and Beardifier collection/compute; no generated-structure or full-world parity claim");
        output.put("primitives",oracle.primitives()); output.put("cases",cases);
        Files.writeString(Path.of(args[1]),new Gson().toJson(output));
        System.out.println("captured " + cases.size() + " native collection fixtures");
    }
}
