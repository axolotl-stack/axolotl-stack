package nativependingprobe;

import java.net.URL;
import java.net.URLClassLoader;

/** Keep helper classes outside the native jar's signed default package. */
public final class Launcher {
    public static void main(String[] args) throws Exception {
        URL source=Launcher.class.getProtectionDomain().getCodeSource().getLocation();
        try(URLClassLoader loader=new URLClassLoader(new URL[]{source},Launcher.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name,boolean resolve)throws ClassNotFoundException {
                boolean helper=false;
                for(String probe:new String[]{"NativePendingActorProbe","NativeWorldgenOracle","NativeContextProbe","NativeContextProbeSnapshot"})
                    if(name.equals(probe)||name.startsWith(probe+"$"))helper=true;
                if(!helper)return super.loadClass(name,resolve);
                Class<?> loaded=findLoadedClass(name);
                if(loaded==null)loaded=findClass(name);
                if(resolve)resolveClass(loaded);
                return loaded;
            }
        }) {
            loader.loadClass("NativePendingActorProbe").getMethod("main",String[].class).invoke(null,(Object)args);
        }
    }
}
