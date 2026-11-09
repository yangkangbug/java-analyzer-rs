// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class A {}
class B extends A {}
interface Task { void run(); }
class Parent { static double /*D:inherited_double*/inherited(double a) { return a; } }
class Boundaries {
    static <T> T /*D:generic_one*/generic(T a) { return a; }
    static long /*D:generic_two*/generic(long a, long b) { return a; }
    static float /*D:spread_float*/spread(float... a) { return a[0]; }
    static double /*D:spread_double*/spread(double... a) { return a[0]; }
    static Long /*D:boxed_long*/boxed(Long a) { return a; }
    static Double /*D:boxed_double*/boxed(Double a) { return a; }
    static long /*D:unbox_long*/unbox(long a) { return a; }
    static double /*D:unbox_double*/unbox(double a) { return a; }
    static A /*D:null_a*/nullable(A a) { return a; }
    static B /*D:null_b*/nullable(B a) { return a; }
    static float /*D:unknown_float*/unknown(float a) { return a; }
    static double /*D:unknown_double*/unknown(double a) { return a; }
    static float give() { return 0; }
    static void /*D:lambda_task*/lambda(Task task) {}
    static void /*D:lambda_long*/lambda(long value) {}
    static float /*D:receiver_float*/receiver(float a) { return a; }
    static double /*D:receiver_double*/receiver(double a) { return a; }
    float /*D:instance_float*/instance(float a) { return a; }
    double /*D:instance_double*/instance(double a) { return a; }
    static <T> T /*D:generic_same_t*/mixedGeneric(T a) { return a; }
    static float /*D:generic_same_float*/mixedGeneric(float a) { return a; }
    static void use(float f, long l, Long boxedValue, A a, Boundaries other, boolean condition) {
        /*Q:generic_refusal*/generic(a);
        /*Q:varargs_refusal*/spread(f, f);
        /*Q:boxing_refusal*/boxed(l);
        /*Q:unboxing_refusal*/unbox(boxedValue);
        /*Q:null_refusal*/nullable(null);
        /*Q:call_expression_refusal*/unknown(give());
        /*Q:conditional_expression_refusal*/unknown(condition ? f : f);
        /*Q:lambda_refusal*/lambda(() -> {});
        other./*Q:receiver_refusal*/receiver(f);
        /*Q:generic_same_arity_refusal*/mixedGeneric(f);
    }
    void instanceUse(float f) { /*Q:instance_refusal*/instance(f); }
}
class InheritanceCalls extends Parent {
    static float /*D:inherited_float*/inherited(float a) { return a; }
    static void use(float f) { /*Q:inheritance_refusal*/inherited(f); }
}
