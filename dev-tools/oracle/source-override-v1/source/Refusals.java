package overrides;
interface Face { default void /*D:face_base*/call(int x) {} }
class InterfaceChild implements Face { public void /*D:face_child*/call(int x) {} }
class Generic<T> { public void /*D:generic_base*/call(int x) {} }
class GenericChild extends Generic<String> { public void /*D:generic_child*/call(int x) {} }
class ThrowsParent { public void /*D:throws_base*/call(int x) throws Exception {} }
class ThrowsChild extends ThrowsParent { public void /*D:throws_child*/call(int x) throws Exception {} }
class NativeParent { public native void /*D:native_base*/call(int x); }
class NativeChild extends NativeParent { public void /*D:native_child*/call(int x) {} }
class ArrayParent { public Product[] /*D:array_base*/array() { return null; } }
class ArrayChild extends ArrayParent { public SubProduct[] /*D:array_child*/array() { return null; } }
class Refusals {
    void run(InterfaceChild i, GenericChild g, ThrowsChild t, NativeChild n, ArrayChild a) throws Exception {
        i./*Q:interface_refusal*/call(1);
        g./*Q:generic_refusal*/call(1);
        t./*Q:throws_refusal*/call(1);
        n./*Q:native_refusal*/call(1);
        a./*Q:array_return_refusal*/array();
    }
}
