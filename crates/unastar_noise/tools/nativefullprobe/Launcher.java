package nativefullprobe;

import java.net.URL;
import java.net.URLClassLoader;
import java.util.Arrays;

/** Isolates only our probes from the signed native jar's default package. */
public final class Launcher {
    private static boolean probeClass(String name) {
        for (String probe : new String[]{"NativeWorldgenOracle", "NativeContextProbe", "NativeContextProbeSnapshot"}) {
            if (name.equals(probe) || name.startsWith(probe + "$")) return true;
        }
        return false;
    }

    public static void main(String[] args) throws Exception {
        URL source = Launcher.class.getProtectionDomain().getCodeSource().getLocation();
        try (URLClassLoader loader = new URLClassLoader(new URL[]{source}, Launcher.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
                // Official native classes retain the original signed parent classpath.
                if (!probeClass(name)) return super.loadClass(name, resolve);
                Class<?> loaded = findLoadedClass(name);
                if (loaded == null) loaded = findClass(name);
                if (resolve) resolveClass(loaded);
                return loaded;
            }
        }) {
            boolean context = args.length > 0 && args[0].equals("--context");
            loader.loadClass(context ? "NativeContextProbe" : "NativeWorldgenOracle")
                .getMethod("main", String[].class)
                .invoke(null, (Object) (context ? Arrays.copyOfRange(args, 1, args.length) : args));
        }
    }
}
