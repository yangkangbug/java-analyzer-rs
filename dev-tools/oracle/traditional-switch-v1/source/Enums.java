// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class Enums {
    enum Mode { /*D:one*/ONE, TWO }
    int run(Mode /*D:mode*/mode) {
        int /*D:enum_result*/result = 0;
        switch (/*Q:enum_selector*/mode) {
            case /*Q:enum_label*/ONE:
                result = 1;
                break;
            default:
                result = 2;
        }
        return /*Q:enum_after*/result;
    }
}
