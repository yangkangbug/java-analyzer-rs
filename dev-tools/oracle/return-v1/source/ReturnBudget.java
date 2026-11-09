// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class ReturnBudget {
    static float /*D:return_budget_float*/pick(float value) { return value; }
    static double /*D:return_budget_double*/pick(double value) { return value; }
    static float identity(float value) { return value; }
    static void use(float value) {
        // Forty nested calls exceed the bounded expression proof depth.
        /*Q:return_budget_refusal*/pick(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(identity(value)))))))))))))))))))))))))))))))))))))))));
    }
}
