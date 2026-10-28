package nativestructureprobe;

import java.net.URL;
import java.net.URLClassLoader;

/** Keeps unsigned probe classes separate from the signed native default package. */
public final class Launcher {
    public static void main(String[] args) throws Exception {
        URL source = Launcher.class.getProtectionDomain().getCodeSource().getLocation();
        try (URLClassLoader loader = new URLClassLoader(new URL[]{source}, Launcher.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
                if (!(name.equals("NativeWorldgenOracle") || name.startsWith("NativeWorldgenOracle$")
                    || name.equals("NativeStructureOracle") || name.startsWith("NativeStructureOracle$"))) {
                    return super.loadClass(name, resolve);
                }
                Class<?> loaded = findLoadedClass(name);
                if (loaded == null) loaded = findClass(name);
                if (resolve) resolveClass(loaded);
                return loaded;
            }
        }) {
            loader.loadClass("NativeStructureOracle").getMethod("main", String[].class).invoke(null, (Object) args);
        }
    }
}
