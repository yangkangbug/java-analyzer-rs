// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package api;
import model.Product;
public class Factory {
 public Product /*D:make*/make(int n) { return new model.Child(); }
 public Product[] /*D:array*/array() { return null; }
 public <T extends Product> T /*D:generic*/generic() { return null; }
}
