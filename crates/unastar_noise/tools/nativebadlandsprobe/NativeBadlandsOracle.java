package nativebadlandsprobe;

import com.google.gson.GsonBuilder;
import java.lang.reflect.*;
import java.net.*;
import java.nio.file.*;
import java.util.*;

/** Constructor-only pinned native surface primitives; never starts a world or server. */
public final class NativeBadlandsOracle {
    private static Object invoke(Object oracle, String name, Class<?>[] signature, Object... args) throws Exception {
        Method method = oracle.getClass().getDeclaredMethod(name, signature);
        method.setAccessible(true);
        return method.invoke(oracle, args);
    }
    private static Object call(Object oracle, String owner, String name, Object receiver, String[] types, Object... args) throws Exception {
        return invoke(oracle, "call", new Class<?>[]{String.class, String.class, Object.class, String[].class, Object[].class}, owner, name, receiver, types, args);
    }
    private static Object field(Object oracle, String owner, String name, Object receiver) throws Exception {
        return invoke(oracle, "instanceField", new Class<?>[]{String.class, String.class, Object.class}, owner, name, receiver);
    }
    private static String canonical(Object oracle, Object state) throws Exception {
        return (String) invoke(oracle, "canonicalState", new Class<?>[]{Object.class}, state);
    }
    public static void main(String[] args) throws Exception {
        if (args.length < 5) throw new IllegalArgumentException("<runtime-root> <compiled-oracle-directory> <output.json> <seed> <x:y:z>...");
        Path runtime = Path.of(args[0]);
        try (URLClassLoader loader = new URLClassLoader(new URL[]{Path.of(args[1]).toUri().toURL()}, NativeBadlandsOracle.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
                if (!name.equals("NativeWorldgenOracle") && !name.startsWith("NativeWorldgenOracle$")) return super.loadClass(name, resolve);
                Class<?> loaded = findLoadedClass(name);
                if (loaded == null) loaded = findClass(name);
                if (resolve) resolveClass(loaded);
                return loaded;
            }
        }) {
            Class<?> oracleClass = loader.loadClass("NativeWorldgenOracle");
            Method verify = oracleClass.getDeclaredMethod("verify", Path.class, String.class, String.class);
            verify.setAccessible(true);
            verify.invoke(null, runtime.resolve("server.jar"), "64bb6d763bed0a9f1d632ec347938594144943ed", "SHA-1");
            verify.invoke(null, runtime.resolve("server-mappings.txt"), "5621e9253f05fd57872bbe7f8ddf5f9a7d525955", "SHA-1");
            verify.invoke(null, runtime.resolve("versions/1.21.11/server-1.21.11.jar"), "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc", "SHA-256");
            Constructor<?> constructor = oracleClass.getDeclaredConstructor(Path.class);
            constructor.setAccessible(true);
            Object oracle = constructor.newInstance(runtime.resolve("server-mappings.txt"));
            long seed = Long.parseLong(args[3]);
            Object state = invoke(oracle, "bootstrap", new Class<?>[]{long.class, String.class, boolean.class}, seed, "overworld", true);
            Method randomAccessor = state.getClass().getDeclaredMethod("random");
            randomAccessor.setAccessible(true);
            Object randomState = randomAccessor.invoke(state);
            String levelgen = "net.minecraft.world.level.levelgen.";
            Object system = call(oracle, levelgen + "RandomState", "surfaceSystem", randomState, new String[]{});
            Object[] bands = (Object[]) field(oracle, levelgen + "SurfaceSystem", "clayBands", system);
            Object factory = field(oracle, levelgen + "SurfaceSystem", "noiseRandom", system);
            Object bandRandom = call(oracle, levelgen + "PositionalRandomFactory", "fromHashOf", factory,
                new String[]{"java.lang.String"}, "minecraft:clay_bands");
            Class<?> randomType = (Class<?>) invoke(oracle, "type", new Class<?>[]{String.class}, "net.minecraft.util.RandomSource");
            List<Object> draws = new ArrayList<>();
            Object traced = java.lang.reflect.Proxy.newProxyInstance(randomType.getClassLoader(), new Class<?>[]{randomType}, (proxy, method, arguments) -> {
                Object result = method.invoke(bandRandom, arguments);
                Map<String, Object> draw = new LinkedHashMap<>();
                draw.put("mapped_method", method.getName());
                draw.put("arguments", arguments == null ? List.of() : Arrays.asList(arguments));
                draw.put("result", result);
                draws.add(draw);
                return result;
            });
            Object[] replayed = (Object[]) call(oracle, levelgen + "SurfaceSystem", "generateBands", system,
                new String[]{"net.minecraft.util.RandomSource"}, traced);
            boolean replayEqual = Arrays.equals(bands, replayed);
            List<String> canonicalBands = new ArrayList<>();
            for (Object band : bands) canonicalBands.add(canonical(oracle, band));
            Object offsetNoise = field(oracle, levelgen + "SurfaceSystem", "clayBandsOffsetNoise", system);
            List<Object> points = new ArrayList<>();
            for (int i = 4; i < args.length; i++) {
                int[] xyz = Arrays.stream(args[i].split(":" )).mapToInt(Integer::parseInt).toArray();
                double offset = (double) call(oracle, levelgen + "synth.NormalNoise", "getValue", offsetNoise,
                    new String[]{"double", "double", "double"}, (double) xyz[0], 0.0, (double) xyz[2]);
                Object band = call(oracle, levelgen + "SurfaceSystem", "getBand", system,
                    new String[]{"int", "int", "int"}, xyz[0], xyz[1], xyz[2]);
                points.add(Map.of("position", xyz, "offset_noise", offset, "offset_noise_f64_bits", String.format("%016x", Double.doubleToRawLongBits(offset)),
                    "rounded_offset", Math.round(offset * 4), "native_get_band", canonical(oracle, band)));
            }
            Map<String, Object> output = new LinkedHashMap<>();
            output.put("profile", "actual tagged vanilla runtime RandomState.surfaceSystem; freshly constructed native clayBands array and native offset noise/getBand calls; no generation, server or reconstructed band table");
            output.put("seed", seed);
            output.put("clay_bands", canonicalBands);
            output.put("factory_seed_lo", field(oracle, levelgen + "XoroshiroRandomSource$XoroshiroPositionalRandomFactory", "seedLo", factory));
            output.put("factory_seed_hi", field(oracle, levelgen + "XoroshiroRandomSource$XoroshiroPositionalRandomFactory", "seedHi", factory));
            output.put("delegated_native_generate_bands_matches_constructed_array", replayEqual);
            output.put("delegated_native_generate_bands_draws", draws);
            output.put("points", points);
            Files.writeString(Path.of(args[2]), new GsonBuilder().setPrettyPrinting().create().toJson(output));
        }
    }
}
