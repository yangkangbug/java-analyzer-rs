package use;
import p.E;
import p.Owner;
import p.Owner.Nested;
public class Use {
    E one = E./*Q:qualified*/A;
    E instance(E value) { return value./*Q:instance_constant*/B; }
    E full() { return p.E./*Q:fully_qualified*/B; }
    int ordinary(E value) { return value./*Q:regular*/ordinary; }
    int chain() { return E./*Q:chain_constant*/A./*Q:chain_field*/ordinary; }
    E shadow(E /*D:parameter_a*/A) { return /*Q:shadowed*/A; }
    Nested nested = Nested./*Q:nested_constant*/C;
    Owner.Nested prefix = Owner.Nested./*Q:nested_prefix*/C;
    E unicode = E./*Q:unicode_constant*/雪;
    void choose(E value) { switch(value) { case /*Q:case_label*/A: break; default: break; } }
}
