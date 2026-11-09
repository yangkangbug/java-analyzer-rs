// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package app;
public class GenericUse<T> extends dep.Generic<T> {
    @Override
    public int /*D:mixed_generic*/generic(T p) { return 1; }
    int probe(T p) { return /*Q:binary_generic_refusal*/generic(p); }
}
