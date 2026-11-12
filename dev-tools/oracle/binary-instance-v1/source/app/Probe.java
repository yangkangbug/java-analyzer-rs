package app;
import dep.*;
// Unicode 🧭 byte-coordinate sentinel
public class Probe {
    public void voidAllowed(Methods m, int x) { m./*Q:void_allowed*/finish(x); }
    public int zero(Methods m) { return m./*Q:zero*/zero(); }
    public int intIdentity(Methods m, int x) { return m./*Q:int_identity*/pick(x); }
    public long longIdentity(Methods m, long x) { return m./*Q:long_identity*/pick(x); }
    public int byteSpecificity(Methods m, byte x) { return m./*Q:byte_specificity*/pick(x); }
    public Token tokenIdentity(Methods m, Token x) { return m./*Q:token_identity*/token(x); }
    public int[] arrayReturn(Methods m, int x) { return m./*Q:array_return*/arrayResult(x); }
    public int finalAllowed(Methods m, int x) { return m./*Q:final_allowed*/finalMethod(x); }
    public int syncAllowed(Methods m, int x) { return m./*Q:sync_allowed*/syncMethod(x); }
    public int nativeAllowed(Methods m, int x) { return m./*Q:native_allowed*/nativeMethod(x); }
    public int abstractAllowed(AbstractOwner m, int x) { return m./*Q:abstract_allowed*/abstractMethod(x); }
    public int dollarBinary(Dollar$Owner m, int x) { return m./*Q:dollar_binary*/ping(x); }
    public int nestedBinary(Outer.Inner m, int x) { return m./*Q:nested_binary*/ping(x); }
    public int staticRefusal(Methods m, int x) { return m./*Q:static_refusal*/staticMethod(x); }
    public int varargRefusal(Methods m) { return m./*Q:vararg_refusal*/vararg(1); }
    public Token genericRefusal(Methods m, Token x) { return m./*Q:generic_refusal*/generic(x); }
    public int arrayFormalRefusal(Methods m, int[] x) { return m./*Q:array_formal_refusal*/arrayFormal(x); }
    public int ancestorRefusal(Child m, int x) { return m./*Q:ancestor_refusal*/inherited(x); }
    public int interfaceRefusal(FaceOwner m, int x) { return m./*Q:interface_refusal*/face(x); }
    public int genericOwnerRefusal(GenericOwner<Token> m, int x) { return m./*Q:generic_owner_refusal*/plain(x); }
    public Token bridgeRefusal(BridgeOwner m, Token x) { return m./*Q:bridge_refusal*/bridge(x); }
    public int staticFieldRefusal() { return Methods.SHARED./*Q:static_field_refusal*/zero(); }
    public int returnReceiverRefusal(Methods m, Token x) { return m.token(x)./*Q:return_receiver_refusal*/id(); }
}
