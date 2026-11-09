// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class NoCallable {
    int field;
    int fieldCase() { return /*Q:field_only_call*/field(); }
    int parameterCase(int parameter) { return /*Q:parameter_only_call*/parameter(); }
    int localCase() {
        int local = 1;
        return /*Q:local_only_call*/local();
    }
    int classCase() { return /*Q:class_only_call*/OtherCallable(); }
    int unrelatedCase() { return /*Q:unrelated_method_call*/unrelated(); }
}
class OtherCallable {
    int unrelated() { return 0; }
}
