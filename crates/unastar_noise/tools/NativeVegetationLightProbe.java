/* Pinned Java 1.21.11 lighting primitives only. Creates no server, level,
 * chunk, task executor, listener, or world directory. Only global static
 * version detection and built-in registry bootstrap precede engine creation.
 * The native engine is sampled before any section data or light work enters it.
 */
package featureprobe;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.Gson;
import static featureprobe.NativeFeaturePrimitives.*;

public final class NativeVegetationLightProbe {
    static final String LIGHT = MC + "world.level.lighting.LevelLightEngine";
    static final String HEIGHT = MC + "world.level.LevelHeightAccessor";
    static final String GETTER = MC + "world.level.BlockGetter";
    static final String SOURCE = CH + "LightChunkGetter";
    static final String POS = MC + "core.BlockPos";

    static Object emptySource() throws Exception {
        Object blocks = Proxy.newProxyInstance(NativeVegetationLightProbe.class.getClassLoader(),
            new Class[]{type(GETTER)}, (proxy, method, args) -> {
                if (matches(method, HEIGHT, "getMinY")) return -64;
                if (matches(method, HEIGHT, "getHeight")) return 384;
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, args);
                throw new UnsupportedOperationException("Unexpected block access: " + method);
            });
        return Proxy.newProxyInstance(NativeVegetationLightProbe.class.getClassLoader(),
            new Class[]{type(SOURCE)}, (proxy, method, args) -> {
                if (matches(method, SOURCE, "getLevel")) return blocks;
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, args);
                throw new UnsupportedOperationException("Unexpected chunk access: " + method);
            });
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 2) throw new IllegalArgumentException("runtime-directory output-json");
        List<String> deniedNetwork = new ArrayList<>();
        System.setSecurityManager(new SecurityManager() {
            public void checkPermission(java.security.Permission permission) {}
            private void deny(String operation) {
                deniedNetwork.add(operation);
                throw new SecurityException("network forbidden: " + operation);
            }
            public void checkConnect(String host, int port) { deny("connect " + host + ":" + port); }
            public void checkListen(int port) { deny("listen " + port); }
            public void checkAccept(String host, int port) { deny("accept " + host + ":" + port); }
        });
        var logging = new org.apache.logging.log4j.core.config.DefaultConfiguration();
        ((Map<String, String>)logging.getComponent("ContextProperties")).put("hostName", "localhost");
        var factory = (org.apache.logging.log4j.core.impl.Log4jContextFactory)org.apache.logging.log4j.LogManager.getFactory();
        factory.getSelector().getContext("NativeVegetationLightProbe", ClassLoader.getSystemClassLoader(), false).start(logging);
        Path runtime = Path.of(args[0]);
        // This constructor only parses the official mappings. Never call its
        // bootstrap/fullstage entry points from this pure primitive observer.
        java.net.URL source = NativeVegetationLightProbe.class.getProtectionDomain().getCodeSource().getLocation();
        java.net.URL helpers = NativeFeaturePrimitives.class.getProtectionDomain().getCodeSource().getLocation();
        ClassLoader loader = new java.net.URLClassLoader(new java.net.URL[]{source, helpers}, NativeVegetationLightProbe.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
                if (!name.equals("NativeWorldgenOracle") && !name.startsWith("NativeWorldgenOracle$")) return super.loadClass(name, resolve);
                Class<?> value = findLoadedClass(name);
                if (value == null) value = findClass(name);
                if (resolve) resolveClass(value);
                return value;
            }
        };
        Constructor<?> constructor = Class.forName("NativeWorldgenOracle", true, loader).getDeclaredConstructor(Path.class);
        constructor.setAccessible(true);
        NativeFeaturePrimitives.oracle = constructor.newInstance(runtime.resolve("server-mappings.txt"));
        for (String[] pin : new String[][]{
            {"server.jar", "64bb6d763bed0a9f1d632ec347938594144943ed", "SHA-1"},
            {"server-mappings.txt", "5621e9253f05fd57872bbe7f8ddf5f9a7d525955", "SHA-1"},
            {"versions/1.21.11/server-1.21.11.jar", "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc", "SHA-256"}
        }) NativeFeaturePrimitives.invoke("verify", new Class[]{Path.class, String.class, String.class}, runtime.resolve(pin[0]), pin[1], pin[2]);
        call(MC + "SharedConstants", "tryDetectVersion", null, new String[]{});
        call(MC + "server.Bootstrap", "bootStrap", null, new String[]{});
        Method rawDefault = method(MC + "world.level.BlockAndTintGetter", "getRawBrightness", POS, "int");
        Method skyDefault = method(MC + "world.level.BlockAndTintGetter", "canSeeSky", POS);
        Class<?> region = type(MC + "server.level.WorldGenRegion");
        Method regionRaw = region.getMethod(rawDefault.getName(), rawDefault.getParameterTypes());
        Method regionSky = region.getMethod(skyDefault.getName(), skyDefault.getParameterTypes());
        if (!regionRaw.getDeclaringClass().equals(type(MC + "world.level.BlockAndTintGetter")))
            throw new IllegalStateException("WorldGenRegion overrides raw brightness: " + regionRaw);
        List<Object> cases = new ArrayList<>();
        for (boolean skylight : new boolean[]{false, true}) {
            Object engine = make(LIGHT, new String[]{SOURCE, "boolean", "boolean"}, emptySource(), true, skylight);
            for (int[] xyz : new int[][]{{0, -65, 0}, {0, -64, 0}, {0, 0, 0}, {0, 319, 0}, {0, 320, 0}, {1606, 69, -1594}, {1611, 73, -1585}}) {
                Object position = pos(xyz[0], xyz[1], xyz[2]);
                for (int darken : new int[]{0, 2, 13, 15}) {
                    int raw = (int)call(LIGHT, "getRawBrightness", engine, new String[]{POS, "int"}, position, darken);
                    cases.add(Map.of("has_skylight", skylight, "pos", List.of(xyz[0], xyz[1], xyz[2]), "sky_darken", darken, "raw_brightness", raw));
                }
            }
            if ((boolean)call(LIGHT, "hasLightWork", engine, new String[]{}))
                throw new IllegalStateException("A read created native lighting work");
        }
        Files.writeString(Path.of(args[1]), new Gson().toJson(Map.of(
            "version", "1.21.11",
            "server_sha1", "64bb6d763bed0a9f1d632ec347938594144943ed",
            "mappings_sha1", "5621e9253f05fd57872bbe7f8ddf5f9a7d525955",
            "profile", "Pure native virgin LevelLightEngine after global static registries; no server/world construction or section initialization; not a natural-world lighting snapshot",
            "region_raw_declaring_class", regionRaw.getDeclaringClass().getName(),
            "region_can_see_sky_declaring_class", regionSky.getDeclaringClass().getName(),
            "denied_network_attempts", deniedNetwork,
            "cases", cases)));
    }
}
