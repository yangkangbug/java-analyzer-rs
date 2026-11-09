// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class SingletonCalls {
    static void /*D:single_box*/box(Long value) {}
    static void /*D:single_spread*/spread(float... values) {}
    static void /*D:single_plain*/plain(float value) {}
    static void /*D:single_empty*/empty() {}
    static float result() { return 0; }
    static void use(long l, float f) {
        /*Q:single_boxing_refusal*/box(l);
        /*Q:single_varargs_refusal*/spread(f, f);
        /*Q:single_zero_varargs_refusal*/spread();
        /*Q:single_result_refusal*/plain(result());
        /*Q:single_identity*/plain(f);
        /*Q:single_zero_arity*/empty();
    }
}
