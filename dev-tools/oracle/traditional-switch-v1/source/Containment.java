// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package oracle;
class Containment {
    int run(int /*D:local_selector*/selector) {
        int /*D:local_result*/result = 0;
        switch (/*Q:local_selector_use*/selector) {
            case 0:
                class Hidden {
                    int /*D:hidden_value*/value;
                    int read() { return /*Q:hidden_use*/value; }
                }
                result = /*Q:outside_hidden*/selector;
                break;
            default: result = 1;
        }
        return /*Q:after_hidden*/result;
    }
}
