package app;
public class Use {
  public void run(dep.Inherited inherited, dep.Override override, dep.Overload overload,
      dep.I direct, dep.J child, dep.Impl impl, String string, CharSequence sequence,
      dep.CovChild covariant, dep.DefaultImpl defaultImpl, dep.GenericChild generic,
      dep.Unrelated unrelated, dep.Cross cross, dep.StaticOver staticOver,
      dep.VarargChild varargs, dep.PublicChild privateAncestor, long wide) {
    inherited./*Q:inherited_base*/pick(1);
    override./*Q:class_override*/pick(1);
    overload./*Q:inherited_overload*/pick(wide);
    overload./*Q:own_overload*/pick(1);
    direct./*Q:abstract_direct*/pick(1);
    child./*Q:abstract_child*/pick(1);
    impl./*Q:class_implements*/pick(1);
    string./*Q:string_length*/length();
    string./*Q:string_char_at*/charAt(0);
    sequence./*Q:sequence_length*/length();
    sequence./*Q:sequence_char_at*/charAt(0);
    covariant./*Q:covariant_refusal*/value();
    defaultImpl./*Q:default_refusal*/value();
    generic./*Q:generic_bridge_refusal*/echo(string);
    unrelated./*Q:unrelated_refusal*/pick(1);
    cross./*Q:cross_branch_refusal*/pick(1);
    direct./*Q:interface_object_refusal*/toString();
    staticOver./*Q:static_competitor_refusal*/pick(wide);
    varargs./*Q:varargs_competitor_refusal*/pick(1);
    privateAncestor./*Q:private_ancestor_refusal*/pick(1);
  }
}
