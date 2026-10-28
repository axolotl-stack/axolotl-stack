/* Optional read-only call instrumentation for pinned 1.21.11 diagnostic
 * runs. Each injected call observes arguments/results and leaves the original
 * operand stack, arguments, native code and return value intact. No RNG draw.
 * Compile with --add-exports java.base/jdk.internal.org.objectweb.asm=ALL-UNNAMED;
 * launch with the same export and -javaagent:<jar containing this class>.
 */
package mossprobe;

import java.lang.instrument.*;
import java.security.ProtectionDomain;
import jdk.internal.org.objectweb.asm.*;

public final class NativeMossAgent {
    public interface Observer {
        void placedStart(Object feature,Object world,Object generator,Object random,Object pos)throws Exception;
        void placedEnd(boolean result)throws Exception;
        void writeStart(Object world,Object pos,Object state)throws Exception;
        void writeEnd(boolean result)throws Exception;
        void biomeStart(Object pos)throws Exception;
        void biomeEnd(Object holder)throws Exception;
    }
    public static volatile Observer observer;
    interface Event { void run(Observer value)throws Exception; }
    static void event(Event event) {
        Observer value=observer;if(value==null)return;
        try{event.run(value);}catch(Exception e){throw new IllegalStateException("native moss observer failed",e);}
    }
    public static void placedStart(Object f,Object w,Object g,Object r,Object p){event(o->o.placedStart(f,w,g,r,p));}
    public static void placedEnd(boolean result){event(o->o.placedEnd(result));}
    public static void writeStart(Object w,Object p,Object s){event(o->o.writeStart(w,p,s));}
    public static void writeEnd(boolean result){event(o->o.writeEnd(result));}
    public static void biomeStart(Object p){event(o->o.biomeStart(p));}
    public static void biomeEnd(Object h){event(o->o.biomeEnd(h));}
    public static void premain(String args,Instrumentation instrumentation) {
        instrumentation.addTransformer(new ClassFileTransformer() {
            public byte[] transform(ClassLoader loader,String name,Class<?> old,ProtectionDomain domain,byte[] bytes) {
                if(!name.equals("fes")&&!name.equals("axn")&&!name.equals("dxq"))return null;
                ClassReader reader=new ClassReader(bytes);
                ClassWriter writer=new ClassWriter(reader,ClassWriter.COMPUTE_MAXS);
                reader.accept(new ClassVisitor(Opcodes.ASM8,writer) {
                    public MethodVisitor visitMethod(int access,String method,String descriptor,String signature,String[] exceptions) {
                        MethodVisitor original=super.visitMethod(access,method,descriptor,signature,exceptions);
                        int kind=name.equals("fes")&&method.equals("b")&&descriptor.equals("(Ldxn;Leqg;Lbgr;Lis;)Z")?1:
                            name.equals("axn")&&method.equals("a")&&descriptor.equals("(Lis;Leoh;II)Z")?2:
                            name.equals("dxq")&&method.equals("a")&&descriptor.equals("(Lis;)Ljd;")?3:0;
                        if(kind==0)return original;
                        return new MethodVisitor(Opcodes.ASM8,original) {
                            public void visitCode() {
                                super.visitCode();
                                if(kind==1){for(int i=0;i<=4;i++)super.visitVarInsn(Opcodes.ALOAD,i);super.visitMethodInsn(Opcodes.INVOKESTATIC,"mossprobe/NativeMossAgent","placedStart","(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)V",false);}
                                if(kind==2){for(int i=0;i<=2;i++)super.visitVarInsn(Opcodes.ALOAD,i);super.visitMethodInsn(Opcodes.INVOKESTATIC,"mossprobe/NativeMossAgent","writeStart","(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)V",false);}
                                if(kind==3){super.visitVarInsn(Opcodes.ALOAD,1);super.visitMethodInsn(Opcodes.INVOKESTATIC,"mossprobe/NativeMossAgent","biomeStart","(Ljava/lang/Object;)V",false);}
                            }
                            public void visitInsn(int opcode) {
                                if((kind<=2&&opcode==Opcodes.IRETURN)||(kind==3&&opcode==Opcodes.ARETURN)) {
                                    super.visitInsn(Opcodes.DUP);
                                    super.visitMethodInsn(Opcodes.INVOKESTATIC,"mossprobe/NativeMossAgent",kind==1?"placedEnd":kind==2?"writeEnd":"biomeEnd",kind==3?"(Ljava/lang/Object;)V":"(Z)V",false);
                                }
                                super.visitInsn(opcode);
                            }
                        };
                    }
                },0);
                return writer.toByteArray();
            }
        });
    }
}
