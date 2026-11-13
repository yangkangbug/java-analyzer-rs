package app;
public class Use {
  public void run(dep.Factory factory, dep.Sink sink, dep.Child child, String string, long wide) {
    sink./*Q:boolean_actual*/pick(factory.booleanValue());
    sink./*Q:byte_actual*/pick(factory.byteValue());
    sink./*Q:char_actual*/pick(factory.charValue());
    sink./*Q:short_actual*/pick(factory.shortValue());
    sink./*Q:int_actual*/pick(factory.intValue());
    sink./*Q:long_actual*/pick(factory.longValue());
    sink./*Q:float_actual*/pick(factory.floatValue());
    sink./*Q:double_actual*/pick(factory.doubleValue());
    factory.value()./*Q:reference_receiver*/ping();
    factory.value().next().next()./*Q:reference_chain*/ping();
    sink./*Q:reference_actual*/accept(factory.value());
    child.inherited()./*Q:inherited_return*/ping();
    factory.choose(wide)./*Q:selected_overload_return*/ping();
    string.trim()./*Q:string_return*/length();
    sink./*Q:array_refusal*/takeObject(factory.array());
    factory.nested()./*Q:nested_refusal*/ping();
    factory.box()./*Q:generic_refusal*/ping();
    factory.staticValue()./*Q:static_refusal*/ping();
    factory.bad(1)./*Q:competitor_refusal*/ping();
    factory.value().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next()./*Q:depth_refusal*/ping();
  }
}
