// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package dep;
public class Base {
    public int scalar(Payload p) { return 0; }
    public int primitive(int p) { return 0; }
    public int array(Payload[] p) { return 0; }
    public int varargs(Payload[] p) { return 0; }
    public int postfix(int[][] p) { return 0; }
    public int distinctPackage(other.Payload p) { return 0; }
    public int distinctPrimitive(long p) { return 0; }
    public int distinctDimensions(Payload[][] p) { return 0; }
}
